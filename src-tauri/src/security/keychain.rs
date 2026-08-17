use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "filemind";
const KEYRING_USER: &str = "api_keys";

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

pub fn get_key(provider: &str) -> AppResult<Option<String>> {
    let keys = load_all_keys()?;
    Ok(keys.get(provider).cloned())
}

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

fn load_all_keys() -> AppResult<std::collections::HashMap<String, String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| AppError::SidecarUnavailable(format!("Keychain 初始化失败: {e}")))?;

    match entry.get_password() {
        Ok(data) => Ok(serde_json::from_str(&data)?),
        Err(_) => Ok(std::collections::HashMap::new()),
    }
}
