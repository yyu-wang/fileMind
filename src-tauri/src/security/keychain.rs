//! API Key 安全存储：密钥只存系统 Keychain，以 JSON 单条目聚合。

use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "filemind";
const KEYRING_USER: &str = "api_keys";

/// 存储指定服务商的 API Key（聚合 JSON 后写入 Keychain）。
///
/// # Errors
///
/// Keychain 初始化、序列化或写入失败时返回错误。
pub fn store_key(provider: &str, key: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 初始化失败: {e}")))?;

    let mut existing = load_all_keys().unwrap_or_default();
    existing.insert(provider.to_string(), key.to_string());
    let serialized = serde_json::to_string(&existing)?;

    entry
        .set_password(&serialized)
        .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 存储失败: {e}")))
}

/// 读取指定服务商的 API Key。
///
/// # Errors
///
/// Keychain 读取或反序列化失败时返回错误；未存储时返回 `None`。
pub fn get_key(provider: &str) -> AppResult<Option<String>> {
    let keys = load_all_keys()?;
    Ok(keys.get(provider).cloned())
}

/// 删除指定服务商的 API Key；全部删空时移除 Keychain 条目。
///
/// # Errors
///
/// Keychain 初始化、序列化或写入失败时返回错误。
pub fn delete_key(provider: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 初始化失败: {e}")))?;

    let mut keys = load_all_keys().unwrap_or_default();
    keys.remove(provider);

    if keys.is_empty() {
        let _ = entry.delete_credential();
    } else {
        let serialized = serde_json::to_string(&keys)?;
        entry
            .set_password(&serialized)
            .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 更新失败: {e}")))?;
    }

    Ok(())
}

/// 从 Keychain 加载全部密钥映射；条目不存在时返回空表。
fn load_all_keys() -> AppResult<std::collections::HashMap<String, String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 初始化失败: {e}")))?;

    match entry.get_password() {
        Ok(data) => Ok(serde_json::from_str(&data)?),
        Err(_) => Ok(std::collections::HashMap::new()),
    }
}
