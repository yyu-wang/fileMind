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

use serde::{Deserialize, Serialize};

use crate::commands::config::CloudProvider;
use crate::security;

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

/// 全部云服务商（§4 凭证清单：`OpenAI` / `DeepSeek`）。
const ALL_PROVIDERS: [CloudProvider; 2] = [CloudProvider::Openai, CloudProvider::Deepseek];

/// 查询所有云服务商的 API Key 状态（仅掩码提示，不含完整 Key）。
///
/// # Errors
///
/// Keychain 读取失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn get_api_key_status() -> Result<Vec<ApiKeyStatus>, String> {
    ALL_PROVIDERS
        .iter()
        .map(|provider| {
            let key = security::get_key(provider_key(*provider))
                .map_err(|e| format!("KEY-001:API Key 读取失败 ({e})"))?;
            Ok(build_status(*provider, key.as_deref()))
        })
        .collect()
}

/// 保存指定云服务商的 API Key（覆盖旧值）。
///
/// # Errors
///
/// Key 校验失败或 Keychain 写入失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn set_api_key(provider: CloudProvider, key: String) -> Result<ApiKeyStatus, String> {
    let key = key.trim();
    validate_key(key)?;
    security::store_key(provider_key(provider), key)
        .map_err(|e| format!("KEY-002:API Key 保存失败 ({e})"))?;
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
    security::delete_key(provider_key(provider))
        .map_err(|e| format!("KEY-003:API Key 删除失败 ({e})"))?;
    Ok(build_status(provider, None))
}

/// `CloudProvider` → Keychain JSON 条目键（与 `config_repo` 的 `SQLite` 字符串一致）。
const fn provider_key(provider: CloudProvider) -> &'static str {
    match provider {
        CloudProvider::Openai => "openai",
        CloudProvider::Deepseek => "deepseek",
    }
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
    key.map_or_else(
        || ApiKeyStatus {
            provider,
            has_key: false,
            hint: String::new(),
        },
        |value| ApiKeyStatus {
            provider,
            has_key: true,
            hint: mask_hint(value),
        },
    )
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
    fn build_status_has_key_and_hint() {
        let status = build_status(CloudProvider::Openai, Some("sk-proj-abcdefghijklmnop"));
        assert!(status.has_key);
        assert_eq!(status.hint, "····mnop");
        assert!(matches!(status.provider, CloudProvider::Openai));
    }

    #[test]
    fn build_status_missing_key_has_empty_hint() {
        let status = build_status(CloudProvider::Deepseek, None);
        assert!(!status.has_key);
        assert_eq!(status.hint, "");
        assert!(matches!(status.provider, CloudProvider::Deepseek));
    }

    #[test]
    fn provider_key_maps_provider_to_entry_key() {
        assert_eq!(provider_key(CloudProvider::Openai), "openai");
        assert_eq!(provider_key(CloudProvider::Deepseek), "deepseek");
    }
}
