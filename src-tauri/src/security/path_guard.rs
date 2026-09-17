//! 路径安全守卫：规范化路径并拦截系统目录黑名单与越界访问。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 系统目录黑名单（BE-M8 扩充）。常量统一小写：匹配时路径同样转小写，
/// 对齐 macOS 大小写不敏感文件系统语义（`/system` 与 `/System` 同路径）。
const BLOCKED_PATTERNS: &[&str] = &[
    "/system",
    "/library",
    "/usr",
    "/bin",
    "/sbin",
    "/dev",
    "/proc",
    "/sys",
    "/etc",
    "/private/etc",
    "/applications",
    "/opt/homebrew",
    "c:\\windows\\system32",
    "c:\\program files",
];

/// 每用户系统目录规则（无法用固定前缀表达，按分段匹配）：
/// macOS `~` = `/Users/<name>`，故 `/Users/<name>/{Library,.ssh}` 拦截；
/// Linux 家目录 `/home/<name>/.ssh` 拦截（home 下的 Library 是用户内容，不拦）。
const PER_USER_RULE_LABEL: &str = "/Users/*/Library 或 ~/.ssh";
const HOME_SSH_RULE_LABEL: &str = "/home/*/.ssh";

/// 校验读路径：规范化并检查黑名单，返回规范化路径。
///
/// # Errors
///
/// 路径不存在、无法解析或命中黑名单时返回 `UnsafePath`。
pub fn validate(path: &str) -> AppResult<PathBuf> {
    let canonical = Path::new(path)
        .canonicalize()
        .map_err(|_| AppError::UnsafePath(format!("路径不存在或无法解析: {path}")))?;

    ensure_not_blocked(path, &canonical)?;

    Ok(canonical)
}

/// 校验路径必须位于指定根目录内（防越界）。
///
/// # Errors
///
/// 同 [`validate`]；此外路径不在根目录内时返回 `UnsafePath`。
pub fn validate_within_root(path: &str, root: &Path) -> AppResult<PathBuf> {
    let canonical = validate(path)?;
    let root_canonical = root
        .canonicalize()
        .map_err(|_| AppError::UnsafePath(format!("根目录无法解析: {}", root.display())))?;

    if !canonical.starts_with(&root_canonical) {
        return Err(AppError::UnsafePath(format!(
            "路径越界: {} 不在根目录 {} 内",
            canonical.display(),
            root_canonical.display()
        )));
    }

    Ok(canonical)
}

/// 校验相对子路径（如分类目标目录 `target_dir`）：只允许正常相对分段。
///
/// 拒绝：空串、绝对路径、Windows 盘符前缀、反斜杠、`.`/`..` 等特殊分段。
/// 返回未规范化的 `PathBuf`（调用方负责与扫描根拼接）。
///
/// # Errors
///
/// 任一非法形式命中时返回 `UnsafePath`。
pub fn validate_relative_subpath(relative: &str) -> AppResult<PathBuf> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(AppError::UnsafePath("相对子路径为空".to_string()));
    }
    if Path::new(trimmed).is_absolute() {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能是绝对路径: {relative}"
        )));
    }
    if trimmed.contains('\\') {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能包含反斜杠: {relative}"
        )));
    }
    // Windows 盘符前缀（C: / C:/x）—— 防御跨平台路径注入
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能包含盘符: {relative}"
        )));
    }
    for component in Path::new(trimmed).components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(AppError::UnsafePath(format!(
                "相对子路径含非法分段: {relative}"
            )));
        }
    }
    Ok(Path::new(trimmed).to_path_buf())
}

/// 校验写目标路径（移动/重命名）：目标本身可不存在。
///
/// 逐级检查路径字符串黑名单，并校验已存在的父目录。
///
/// # Errors
///
/// 目标为空、命中黑名单或父目录无法解析时返回 `UnsafePath`。
pub fn validate_write_target(target: &str) -> AppResult<PathBuf> {
    if target.is_empty() {
        return Err(AppError::UnsafePath("目标路径为空".to_string()));
    }

    let target_path = Path::new(target);

    // 第一道：按原始路径字符串快查（拦截显式黑名单写法与大小写变体）。
    ensure_not_blocked(target, target_path)?;

    if target_path.exists() {
        let canonical = target_path
            .canonicalize()
            .map_err(|_| AppError::UnsafePath(format!("目标路径无法解析: {target}")))?;
        ensure_not_blocked(target, &canonical)?;
        return Ok(target_path.to_path_buf());
    }

    // 第二道（BE-M4）：目标不存在时，沿路径逐级向上找最近存在的祖先目录
    // canonicalize（解析路径中全部符号链接），拼回未创建段后重查黑名单。
    // 否则 /tmp/link/newdir/f.txt（link → /System）这类「目标与父目录都不
    // 存在」的写法只做字符串检查即可绕过，create_dir_all 会穿透符号链接
    // 在黑名单目录内建目录落文件。
    let mut remaining: Vec<std::ffi::OsString> = Vec::new();
    let mut current = target_path;
    while let Some(parent) = current.parent() {
        if parent.as_os_str().is_empty() {
            // 相对路径走完所有上级（如 "foo.txt"），字符串校验已覆盖
            break;
        }
        if parent.exists() {
            let canonical = parent.canonicalize().map_err(|_| {
                AppError::UnsafePath(format!("目标父目录无法解析: {}", parent.display()))
            })?;
            let mut resolved = canonical;
            for seg in remaining.iter().rev() {
                resolved.push(seg);
            }
            ensure_not_blocked(target, &resolved)?;
            break;
        }
        if let Some(name) = current.file_name() {
            remaining.push(name.to_os_string());
        }
        current = parent;
    }

    Ok(target_path.to_path_buf())
}

/// 路径命中黑名单时返回 `UnsafePath`（display 用于错误信息展示原始写法）。
fn ensure_not_blocked(display: &str, path: &Path) -> AppResult<()> {
    if let Some(pattern) = blocked_match(path) {
        return Err(AppError::UnsafePath(format!(
            "路径被安全策略阻止: {display} 匹配黑名单 {pattern}"
        )));
    }
    Ok(())
}

/// 分段级黑名单匹配：返回命中的规则标签。
///
/// 比较统一小写（macOS 大小写不敏感语义），且要求段边界对齐——
/// `/etc` 拦截 `/etc/hosts` 但不误伤 `/etcetera`。
fn blocked_match(path: &Path) -> Option<&'static str> {
    let lower = path.to_string_lossy().to_lowercase();
    for pattern in BLOCKED_PATTERNS {
        // 边界分隔符跟随规则本身的风格（Windows 规则用 '\'，其余用 '/'）
        let boundary = if pattern.contains('\\') {
            format!("{pattern}\\")
        } else {
            format!("{pattern}/")
        };
        if lower == *pattern || lower.starts_with(&boundary) {
            return Some(pattern);
        }
    }

    // 每用户系统目录规则：按分段判断（第 1 段 = users/home，第 3 段 = 目标名）
    let segs: Vec<&str> = lower.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() >= 3 {
        if segs[0] == "users" && (segs[2] == "library" || segs[2] == ".ssh") {
            return Some(PER_USER_RULE_LABEL);
        }
        if segs[0] == "home" && segs[2] == ".ssh" {
            return Some(HOME_SSH_RULE_LABEL);
        }
    }
    None
}

// 单元测试移到兄弟文件 path_guard_tests.rs（原内嵌，与实现合计 395 行）。

#[cfg(test)]
#[path = "path_guard_tests.rs"]
mod tests;
