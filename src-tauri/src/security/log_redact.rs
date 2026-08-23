//! 日志脱敏过滤器（安全 I-03 / rules/observability.md / rules/security.md）。
//!
//! 设计为日志系统的「单一出口」过滤层：Rust 侧挂在 `env_logger` 自定义 formatter，
//! Python 侧挂在 `core/logging.py::getLogger` 包装器（见 `python-sidecar`），保证新增
//! 日志语句即使忘记手动脱敏，也不会泄漏 API Key / 绝对路径。
//!
//! 脱敏规则（顺序敏感，先密钥后路径）：
//! - API Key / Bearer Token / 常见密钥型 `key=value` → `***REDACTED***`
//! - POSIX / Windows 绝对路径 → 仅保留末级（`/Users/x/secret.pdf` → `***/secret.pdf`）
//! - URL（`http://…`）与单段系统路径（`/tmp`、`/health`）不受影响
//!
//! 用户查询内容（RAG 问题）不在此处识别——自由文本无法靠正则判定，规范要求是
//! 「不记录原始内容」，由调用侧保证（当前 chat 链路确实未记录 query，见
//! `commands/chat.rs` / `routes_chat.py`）。

use std::sync::OnceLock;

use regex::{Captures, Regex};

/// 密钥/令牌型泄漏的统一替换占位符。
const REDACTED: &str = "***REDACTED***";

/// 路径脱敏后保留末级时的前缀占位符。
const PATH_PREFIX: &str = "***";

/// 编译好的密钥型正则集合（`init()` 时一次性构建，之后只读）。
static KEY_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
/// 编译好的路径型正则集合（`init()` 时一次性构建，之后只读）。
static PATH_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

/// 密钥型泄漏正则（无 lookaround —— regex crate 不支持；靠后处理/结构规避）。
fn build_key_patterns() -> Result<Vec<Regex>, String> {
    [
        // OpenAI 风格密钥 sk-…（{16,} 避免误伤短串）
        Regex::new(r"sk-[A-Za-z0-9_-]{16,}"),
        // Authorization: Bearer <token>（token 可含 / 与 =，须在路径处理前整体替换）
        Regex::new(r"(?i)bearer[ =:]+[A-Za-z0-9._~+/=-]{8,}"),
        // 常见密钥型 key=value / key: value（r#"..."# 为容纳模式中的双引号）
        Regex::new(r#"(?i)(?:api[_-]?key|access[_-]?token|secret|password)[ =:]+["']?[A-Za-z0-9._~+/=-]{6,}"#),
    ]
    .into_iter()
    .map(|res| res.map_err(|e| format!("密钥正则编译失败: {e}")))
    .collect()
}

/// 绝对路径型正则（POSIX 与 Windows 盘符两种写法）。
///
/// 段内字符排除空白与常见标点：既保证能匹配含中文的文件路径，又避免跨行/跨词
/// 贪心吞掉整句上下文。含空格的路径由调用侧 `sanitize_path` 兜底（层级过滤只做
/// 保守匹配，宁可漏也不误伤）。
fn build_path_patterns() -> Result<Vec<Regex>, String> {
    [
        // POSIX：/a/b/…（≥2 段）。单段 `/tmp`、URL 路径 `/shutdown` 不匹配
        Regex::new(r#"/(?:[^/\s"'(),;:]+/)+[^/\s"'(),;:]+"#),
        // Windows：C:\a\b\…（盘符 + 至少 1 段）
        Regex::new(r#"[A-Za-z]:\\(?:[^\\\s"'(),;:]+)(?:\\[^\\\s"'(),;:]+)*"#),
        // Windows 前斜杠写法 C:/a/b/…
        Regex::new(r#"[A-Za-z]:/(?:[^/\s"'(),;:]+)(?:/[^/\s"'(),;:]+)+"#),
    ]
    .into_iter()
    .map(|res| res.map_err(|e| format!("路径正则编译失败: {e}")))
    .collect()
}

/// 初始化脱敏正则集合。
///
/// 必须在 `env_logger` 初始化前调用（formatter 依赖正则已就绪）。模式为编译期常量，
/// 正常不会失败；失败时返回错误由调用方决定是否以非零码退出（不进入无脱敏运行态）。
/// 重复调用幂等（`OnceLock::set` 失败被忽略）。
///
/// # Errors
///
/// 任一正则编译失败时返回描述性错误。
pub fn init() -> Result<(), String> {
    let keys = build_key_patterns()?;
    let paths = build_path_patterns()?;
    let _ = KEY_PATTERNS.set(keys);
    let _ = PATH_PATTERNS.set(paths);
    Ok(())
}

/// 把日志消息中的敏感信息统一替换为占位符。
///
/// 顺序敏感：先处理密钥/令牌（可能含 `/`，须在路径处理前整体替换为
/// `***REDACTED***`），再把绝对路径收敛为 `***/末级`。正则未就绪时原样返回
/// （仅测试未调用 `init` 时出现，正常运行路径由 `init()` 保证）。
pub fn redact(input: &str) -> String {
    let Some(keys) = KEY_PATTERNS.get() else {
        return input.to_string();
    };

    let mut out = input.to_string();
    for re in keys {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    if let Some(paths) = PATH_PATTERNS.get() {
        for re in paths {
            out = re
                .replace_all(&out, |caps: &Captures| sanitize_path(&caps[0]))
                .into_owned();
        }
    }
    out
}

/// 路径脱敏：仅保留最后一级，前缀替换为 `***`。
///
/// 同时兼容 POSIX（`/`）与 Windows（`\`）分隔符；空段（末尾斜杠）被忽略。
/// 例：`/Users/x/secret.pdf` → `***/secret.pdf`，`C:\docs\a.txt` → `***/a.txt`。
#[must_use]
pub fn sanitize_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit('/')
        .find(|seg| !seg.is_empty())
        .map_or_else(
            || PATH_PREFIX.to_string(),
            |last| format!("{PATH_PREFIX}/{last}"),
        )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// 全部单测共享：redact 依赖 init 后的正则集合。
    fn setup() {
        init().expect("测试用正则必须可编译");
    }

    #[test]
    fn redact_masks_openai_style_key() {
        setup();
        let msg = "请求失败 key=sk-abcdefghijklmnopqrstuvwxyz123456";
        let out = redact(msg);
        assert!(out.contains(REDACTED));
        assert!(!out.contains("sk-abcdefghijklmnopqrstuvwxyz123456"));
    }

    #[test]
    fn redact_masks_bearer_token() {
        setup();
        let msg = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature_value_1";
        let out = redact(msg);
        assert!(out.contains(REDACTED));
        assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn redact_masks_key_value_forms() {
        setup();
        let msg = "api_key=supersecret12345, password = hunter2value, secret: mysecret999";
        let out = redact(msg);
        assert!(!out.contains("supersecret12345"));
        assert!(!out.contains("hunter2value"));
        assert!(!out.contains("mysecret999"));
        assert_eq!(out.matches(REDACTED).count(), 3);
    }

    #[test]
    fn redact_masks_posix_absolute_path() {
        setup();
        let msg = "处理完成: /Users/wangyu/Desktop/report.pdf";
        let out = redact(msg);
        assert!(out.contains("***/report.pdf"));
        assert!(!out.contains("/Users/wangyu"));
    }

    #[test]
    fn redact_masks_windows_absolute_path() {
        setup();
        let msg = r"读取失败: C:\Users\wangyu\docs\a.txt";
        let out = redact(msg);
        assert!(out.contains("***/a.txt"));
        assert!(!out.contains(r"C:\Users\wangyu"));
    }

    #[test]
    fn redact_preserves_url_and_single_segment_paths() {
        setup();
        let msg = "sidecar http://127.0.0.1:8765/health 检查 /tmp 目录";
        let out = redact(msg);
        assert!(
            out.contains("http://127.0.0.1:8765/health"),
            "URL 不应被脱敏: {out}"
        );
        assert!(out.contains("/tmp"), "单段系统路径不应被脱敏: {out}");
    }

    #[test]
    fn redact_masks_path_inside_quoted_error() {
        setup();
        let msg = "error=\"No such file: /home/user/项目文档/收支表.xlsx\", code=E_NOENT";
        let out = redact(msg);
        assert!(out.contains("***/收支表.xlsx"));
        assert!(!out.contains("/home/user"));
    }

    #[test]
    fn redact_leaves_ordinary_log_text_intact() {
        setup();
        let msg = "扫描完成 files=42 duration_ms=350 mode=local";
        let out = redact(msg);
        assert_eq!(out, msg);
    }

    #[test]
    fn sanitize_path_handles_slashes_and_edges() {
        assert_eq!(sanitize_path("/Users/x/docs"), "***/docs");
        assert_eq!(sanitize_path(r"C:\a\b\c.txt"), "***/c.txt");
        assert_eq!(sanitize_path("/a/b/"), "***/b");
        assert_eq!(sanitize_path("/"), "***");
        assert_eq!(sanitize_path(""), "***");
    }
}
