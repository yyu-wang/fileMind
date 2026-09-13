//! 云端 API Key 存取命令（安全 07-§4 / T7.3）。
//!
//! Key 只存系统 Keychain（macOS Keychain / Windows Credential Manager /
//! Linux Secret Service），见 `crate::security::keychain`。命令层遵循 §4
//! 生命周期：用户输入 → Keychain → 使用时由 Rust 侧读取（T7.4 代理转发时用
//! `crate::security::get_key`）；撤回同意仅清内存，Keychain 保留可手动删除。
//!
//! 安全约束：**任何命令都不回传完整 Key**——前端仅见 `has_key` 与末 4 位掩码
//! `hint`；完整 Key 只在 Rust 内部读取。配合 T7.1 日志脱敏过滤（`log_redact`）
//! 双层兜底，日志与 renderer 均不落明文。
//!
//! P-07 自定义提供商演进：`CloudProvider` 已从编译期枚举升级为用户自定义
//! 字符串（`provider_key`）。`get_api_key_status` 不再硬编码列表，改为从
//! `cloud_providers` 表读取用户配置的全部提供商；写入命令新增 slug 校验，
//! 阻止 Keychain 键名带空格/斜杠等脏值。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::config::{validate_provider_key, CloudProvider};
use crate::db::ConfigRepo;
use crate::error::AppResult;
use crate::security;
use crate::AppState;

/// 单个云服务商的 API Key 状态（不含完整 Key）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ApiKeyStatus {
    /// 云服务商。
    pub provider: CloudProvider,
    /// 是否已配置 Key。
    pub has_key: bool,
    /// 掩码提示（`····{末4位}`）；未配置时为空串。
    pub hint: String,
}

/// 查询所有云服务商的 API Key 状态（仅掩码提示，不含完整 Key）。
///
/// 列表不再硬编码：从 `cloud_providers` 表读取所有未删除的 `provider_key`
/// 逐个查 Keychain；若表为空（异常脏数据），返回空向量不报错——由前端提示无
/// 提供商、引导用户新建。
///
/// # 错误降级（用户体验优化）
/// 仅打开设置页查看时，若用户拒绝 Keychain 访问或 Keychain 临时锁定，
/// **不向上层报错**——降级为所有 provider `has_key=false`（视为未配置），
/// 只记 warning 日志。写入/删除（`set_api_key` / `delete_api_key`）与云端
/// 请求才强制要求 Keychain 可用并返回错误，避免打开设置页就被 Keychain
/// 弹窗与错误 toast 骚扰。
///
/// # Errors
/// 本实现已将 Keychain 读取失败全部降级为空状态并以 `Ok` 返回；`Result::Err`
/// 分支仅作为 IPC 类型契约保留（若未来新增错误路径可在此展开）。
#[tauri::command(async)]
#[specta::specta]
pub fn get_api_key_status(state: State<'_, AppState>) -> Result<Vec<ApiKeyStatus>, String> {
    let providers: Vec<CloudProvider> = {
        let db = state
            .db
            .lock()
            .map_err(|e| format!("DB-AK-001:数据库读取失败 ({e})"))?;
        ConfigRepo::list_provider_keys(db.conn()).map_err(|e| e.to_string())?
    };
    let mut any_failed = false;
    let statuses: Vec<ApiKeyStatus> = providers
        .iter()
        .map(|provider| match security::get_key(provider) {
            Ok(key) => build_status(provider.clone(), key.as_deref()),
            Err(e) => {
                any_failed = true;
                log::warn!(
                    "get_api_key_status: Keychain 读取失败，降级为未配置 (provider={provider}, err={e})"
                );
                build_status(provider.clone(), None)
            }
        })
        .collect();
    if any_failed {
        log::warn!(
            "get_api_key_status: 存在 Keychain 访问失败，已全部降级为空状态；\
             若需要保存/使用 Key，请允许 Keychain 访问并重试"
        );
    }
    Ok(statuses)
}

/// 保存指定云服务商的 API Key（覆盖旧值）。
///
/// # Errors
///
/// Key 校验失败或 Keychain 写入失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn set_api_key(provider: CloudProvider, key: String) -> Result<ApiKeyStatus, String> {
    validate_provider_key(&provider)?;
    let key = key.trim();
    validate_key(key)?;
    security::store_key(&provider, key).map_err(|e| format!("KEY-002:API Key 保存失败 ({e})"))?;
    // 07-§4 审计：只记 provider，绝不写 Key 内容（T7.1 日志脱敏二次兜底）
    log::info!("security.api_key: 已更新 provider={provider}");
    Ok(build_status(provider, Some(key)))
}

/// 删除指定云服务商的 API Key。
///
/// # Errors
///
/// Keychain 写入失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn delete_api_key(provider: CloudProvider) -> Result<ApiKeyStatus, String> {
    validate_provider_key(&provider)?;
    security::delete_key(&provider).map_err(|e| format!("KEY-003:API Key 删除失败 ({e})"))?;
    Ok(build_status(provider, None))
}

/// 校验 API Key：非空（去除首尾空白）且长度 8–512 字符。
fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("KEY-100:API Key 不能为空".to_string());
    }
    if !(8..=512).contains(&key.chars().count()) {
        return Err("KEY-101:API Key 长度应为 8–512 字符".to_string());
    }
    Ok(())
}

/// 掩码提示：仅暴露末 4 位（`····abcd`）；过短时整体掩码为 `****`。
fn mask_hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 4 {
        return "****".to_string();
    }
    let last4: String = chars[chars.len() - 4..].iter().collect();
    format!("····{last4}")
}

/// 依据当前 Key 构建状态（无 Key 时 `has_key=false`、`hint` 为空串）。
fn build_status(provider: CloudProvider, key: Option<&str>) -> ApiKeyStatus {
    let has_key = key.is_some();
    let hint = key.map_or_else(String::new, mask_hint);
    ApiKeyStatus {
        provider,
        has_key,
        hint,
    }
}

/// 在 `ConfigRepo` 上扩展：列出未删除提供商的 `provider_key` 清单。
///
/// 放在这里而非 `config_repo.rs`：逻辑极小且是 `api_key` 模块专用，避免污染
/// `config_repo` 公共面；用 `AppResult` 保持与 repo 层一致的错误语义。
#[allow(dead_code)] // 仅本文件通过 db lock 调用，dead_code 误报
fn list_provider_keys(conn: &rusqlite::Connection) -> AppResult<Vec<CloudProvider>> {
    let mut stmt = conn.prepare(
        "SELECT provider_key FROM cloud_providers WHERE is_deleted = 0 ORDER BY is_builtin DESC, updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(row?);
    }
    Ok(keys)
}

// 把 list_provider_keys 挂到 ConfigRepo impl 外作为独立函数复用不到，
// 这里在文件尾直接用上面的局部函数即可；get_api_key_status 用的是
// ConfigRepo::list_provider_keys，所以再补一个真正的关联函数。
impl ConfigRepo {
    /// 列出所有未删除云提供商的 `provider_key`（供 API Key 状态、代理路由使用）。
    ///
    /// # Errors
    ///
    /// 表不存在（迁移未跑）或查询失败时按 `AppError` 回传；若云端尚未建表，
    /// 返回空向量以兼容开发环境手动跳过迁移的场景（实际生产迁移先于此命令）。
    pub fn list_provider_keys(conn: &rusqlite::Connection) -> AppResult<Vec<CloudProvider>> {
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cloud_providers'",
            [],
            |row| row.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if !table_exists {
            return Ok(Vec::new());
        }
        list_provider_keys(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_hint_reveals_only_last_four() {
        assert_eq!(mask_hint("sk-proj-abcd1234wxyz"), "····wxyz");
        assert_eq!(mask_hint("abcd"), "····abcd");
        assert_eq!(mask_hint("abc"), "****");
    }

    #[test]
    fn validate_key_rejects_blank_and_bounds() {
        assert!(validate_key("").is_err());
        assert!(validate_key("   ").is_err());
        assert!(validate_key("short").is_err()); // <8
        assert!(validate_key(&"x".repeat(600)).is_err()); // >512
        assert!(validate_key("sk-abcdefghijklmnopqrstuvwxyz").is_ok());
    }

    #[test]
    fn validate_provider_key_accepts_slug_like_strings() {
        assert!(validate_provider_key("openai").is_ok());
        assert!(validate_provider_key("deepseek-cn").is_ok());
        assert!(validate_provider_key("silicon_flow_v2").is_ok());
        assert!(validate_provider_key("123").is_ok());
        // 拒绝空/超长/大写/路径片段
        assert!(validate_provider_key("").is_err());
        assert!(validate_provider_key(&"a".repeat(65)).is_err());
        assert!(validate_provider_key("OpenAI").is_err());
        assert!(validate_provider_key("my provider").is_err());
        assert!(validate_provider_key("../etc").is_err());
        assert!(validate_provider_key("api.example.com").is_err());
    }

    #[test]
    fn build_status_has_key_and_hint() {
        let status = build_status("openai".to_string(), Some("sk-proj-abcdefghijklmnop"));
        assert!(status.has_key);
        assert_eq!(status.hint, "····mnop");
        assert_eq!(status.provider, "openai");
    }

    #[test]
    fn build_status_missing_key_has_empty_hint() {
        let status = build_status("deepseek".to_string(), None);
        assert!(!status.has_key);
        assert_eq!(status.hint, "");
        assert_eq!(status.provider, "deepseek");
    }
}

// lint fix notes: doc_markdown (api_key / config_repo 反引号)
