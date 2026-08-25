//! API Key 安全存储：密钥只存系统 Keychain，以 JSON 单条目聚合。
//!
//! 安全规则（BE-C3）：聚合写回前必须成功读取存量密钥——读失败（如
//! Keychain 锁定、拒绝访问）时立即中止并返回错误，绝不能用空表兜底，
//! 否则一次写回会静默清空全部已存密钥。

use std::collections::HashMap;

use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "filemind";
const KEYRING_USER: &str = "api_keys";

/// 密钥存储三原语，抽象出来便于单测注入 mock（不触真实系统 Keychain）。
trait CredentialStore {
    /// 读取聚合 JSON 字符串；条目不存在时返回 `Ok(None)`。
    ///
    /// # Errors
    ///
    /// 存储访问失败（锁定/平台故障等）时返回 `AppError::Keychain`。
    fn get(&self) -> AppResult<Option<String>>;
    /// 写入聚合 JSON 字符串。
    ///
    /// # Errors
    ///
    /// 写入失败时返回 `AppError::Keychain`。
    fn set(&self, value: &str) -> AppResult<()>;
    /// 删除整个条目。
    ///
    /// # Errors
    ///
    /// 删除失败时返回 `AppError::Keychain`；条目不存在返回 `Ok(false)`（幂等）。
    fn delete(&self) -> AppResult<bool>;
}

/// 基于 `keyring` crate 的真实系统密钥链实现。
struct SystemKeyring {
    entry: keyring::Entry,
}

impl SystemKeyring {
    /// 创建指向固定服务/用户名的 Keychain 条目句柄。
    ///
    /// # Errors
    ///
    /// 平台密钥链初始化失败时返回 `AppError::Keychain`。
    fn new() -> AppResult<Self> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| AppError::Keychain(format!("Keychain 初始化失败: {e}")))?;
        Ok(Self { entry })
    }
}

impl CredentialStore for SystemKeyring {
    fn get(&self) -> AppResult<Option<String>> {
        match self.entry.get_password() {
            Ok(data) => Ok(Some(data)),
            // NoEntry 是唯一可视为「无密钥」的错误；其余（锁定/平台故障/
            // 存储格式损坏等）必须向上传播，防止调用方以空表兜底覆盖写入。
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Keychain(format!("Keychain 读取失败: {e}"))),
        }
    }

    fn set(&self, value: &str) -> AppResult<()> {
        self.entry
            .set_password(value)
            .map_err(|e| AppError::Keychain(format!("Keychain 存储失败: {e}")))
    }

    fn delete(&self) -> AppResult<bool> {
        match self.entry.delete_credential() {
            Ok(()) => Ok(true),
            // 条目本就不存在：视为幂等删除成功。
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(AppError::Keychain(format!("Keychain 删除失败: {e}"))),
        }
    }
}

/// 存储指定服务商的 API Key（聚合 JSON 后写入 Keychain）。
///
/// # Errors
///
/// Keychain 初始化、读取、序列化或写入失败时返回错误；读失败时中止写入。
pub fn store_key(provider: &str, key: &str) -> AppResult<()> {
    let store = SystemKeyring::new()?;
    store_key_impl(&store, provider, key)
}

/// 读取指定服务商的 API Key。
///
/// # Errors
///
/// Keychain 读取或反序列化失败时返回错误；未存储时返回 `None`。
pub fn get_key(provider: &str) -> AppResult<Option<String>> {
    let store = SystemKeyring::new()?;
    get_key_impl(&store, provider)
}

/// 删除指定服务商的 API Key；全部删空时移除 Keychain 条目。
///
/// # Errors
///
/// Keychain 初始化、读取、序列化、写入或删除失败时返回错误。
pub fn delete_key(provider: &str) -> AppResult<()> {
    let store = SystemKeyring::new()?;
    delete_key_impl(&store, provider)
}

/// [`store_key`] 的核心实现（接收注入的存储，便于单测）。
fn store_key_impl(store: &dyn CredentialStore, provider: &str, key: &str) -> AppResult<()> {
    // 关键：读失败直接中止（错误向上传播），绝不退化成空表继续写回，
    // 否则会把其他服务商的已有密钥静默清空（BE-C3）。
    let mut existing = load_all_keys_impl(store)?;
    existing.insert(provider.to_string(), key.to_string());
    let serialized = serde_json::to_string(&existing)?;
    store.set(&serialized)
}

/// [`get_key`] 的核心实现。
fn get_key_impl(store: &dyn CredentialStore, provider: &str) -> AppResult<Option<String>> {
    Ok(load_all_keys_impl(store)?.get(provider).cloned())
}

/// [`delete_key`] 的核心实现。
fn delete_key_impl(store: &dyn CredentialStore, provider: &str) -> AppResult<()> {
    let mut keys = load_all_keys_impl(store)?;
    keys.remove(provider);

    if keys.is_empty() {
        // 删除条目的失败不允许吞掉（BE-m2）：残留密钥却报成功会误导用户。
        store.delete()?;
    } else {
        let serialized = serde_json::to_string(&keys)?;
        store.set(&serialized)?;
    }

    Ok(())
}

/// 从存储加载全部密钥映射；条目不存在时返回空表，其他读错误向上传播。
fn load_all_keys_impl(store: &dyn CredentialStore) -> AppResult<HashMap<String, String>> {
    match store.get()? {
        Some(data) => Ok(serde_json::from_str(&data).map_err(|e| {
            // 存量 JSON 损坏同样必须中止（返回错误），静默当空表会引发覆盖清空。
            AppError::Keychain(format!("Keychain 存量数据解析失败: {e}"))
        })?),
        None => Ok(HashMap::new()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// 可编程 mock 存储：可模拟读/删失败，并记录 set/delete 是否被调用。
    struct MockStore {
        data: std::cell::RefCell<Option<String>>,
        fail_get: bool,
        fail_delete: bool,
        set_called: std::cell::Cell<bool>,
        delete_called: std::cell::Cell<bool>,
    }

    impl MockStore {
        fn with_data(data: Option<&str>) -> Self {
            Self {
                data: std::cell::RefCell::new(data.map(str::to_string)),
                fail_get: false,
                fail_delete: false,
                set_called: std::cell::Cell::new(false),
                delete_called: std::cell::Cell::new(false),
            }
        }

        /// 模拟 Keychain 锁定/拒绝访问（NoStorageAccess 场景）。
        fn failing_get(data: Option<&str>) -> Self {
            Self {
                fail_get: true,
                ..Self::with_data(data)
            }
        }

        /// 模拟条目删除失败（密钥残留场景）。
        fn failing_delete(data: Option<&str>) -> Self {
            Self {
                fail_delete: true,
                ..Self::with_data(data)
            }
        }
    }

    impl CredentialStore for MockStore {
        fn get(&self) -> AppResult<Option<String>> {
            if self.fail_get {
                return Err(AppError::Keychain(
                    "模拟读取失败（Keychain 锁定）".to_string(),
                ));
            }
            Ok(self.data.borrow().clone())
        }

        fn set(&self, value: &str) -> AppResult<()> {
            self.set_called.set(true);
            *self.data.borrow_mut() = Some(value.to_string());
            Ok(())
        }

        fn delete(&self) -> AppResult<bool> {
            self.delete_called.set(true);
            if self.fail_delete {
                return Err(AppError::Keychain("模拟删除失败".to_string()));
            }
            let existed = self.data.borrow().is_some();
            *self.data.borrow_mut() = None;
            Ok(existed)
        }
    }

    #[test]
    fn store_key_read_failure_aborts_without_writing() {
        // BE-C3 核心回归：读失败（模拟锁定）时必须报错且绝不写回，
        // 否则已有密钥会被空表覆盖清空。
        let store = MockStore::failing_get(Some(r#"{"openai":"sk-existing"}"#));
        let result = store_key_impl(&store, "deepseek", "sk-new-key-1");
        assert!(matches!(result, Err(AppError::Keychain(_))));
        assert!(!store.set_called.get(), "读失败时绝不允许触发写入");
    }

    #[test]
    fn store_key_first_write_creates_entry() {
        let store = MockStore::with_data(None);
        store_key_impl(&store, "openai", "sk-first-key").unwrap();
        assert!(store.set_called.get());
        let written: HashMap<String, String> =
            serde_json::from_str(&store.data.borrow().clone().unwrap()).unwrap();
        assert_eq!(
            written.get("openai").map(String::as_str),
            Some("sk-first-key")
        );
    }

    #[test]
    fn store_key_merges_and_preserves_other_providers() {
        let store = MockStore::with_data(Some(r#"{"openai":"sk-keep-me"}"#));
        store_key_impl(&store, "deepseek", "sk-new-key-2").unwrap();
        let written: HashMap<String, String> =
            serde_json::from_str(&store.data.borrow().clone().unwrap()).unwrap();
        assert_eq!(
            written.get("openai").map(String::as_str),
            Some("sk-keep-me")
        );
        assert_eq!(
            written.get("deepseek").map(String::as_str),
            Some("sk-new-key-2")
        );
    }

    #[test]
    fn store_key_corrupted_existing_data_aborts() {
        // 存量 JSON 损坏同样中止，不允许静默当空表覆盖。
        let store = MockStore::with_data(Some("{not-json"));
        let result = store_key_impl(&store, "openai", "sk-anything");
        assert!(matches!(result, Err(AppError::Keychain(_))));
        assert!(!store.set_called.get());
    }

    #[test]
    fn delete_key_keeps_remaining_providers() {
        let store = MockStore::with_data(Some(r#"{"openai":"sk-a","deepseek":"sk-b"}"#));
        delete_key_impl(&store, "openai").unwrap();
        assert!(!store.delete_called.get(), "仍有剩余密钥时不应删除条目");
        let written: HashMap<String, String> =
            serde_json::from_str(&store.data.borrow().clone().unwrap()).unwrap();
        assert_eq!(written.len(), 1);
        assert!(written.contains_key("deepseek"));
    }

    #[test]
    fn delete_key_last_provider_removes_entry() {
        let store = MockStore::with_data(Some(r#"{"openai":"sk-last"}"#));
        delete_key_impl(&store, "openai").unwrap();
        assert!(store.delete_called.get());
        assert!(store.data.borrow().is_none());
    }

    #[test]
    fn delete_key_missing_entry_is_idempotent_success() {
        // 条目本不存在：删除视为幂等成功（BE-m2 的例外条款）。
        let store = MockStore::with_data(None);
        let result = delete_key_impl(&store, "openai");
        assert!(result.is_ok());
    }

    #[test]
    fn delete_key_entry_removal_failure_propagates() {
        // BE-m2：删空后条目删除失败必须报错，不得静默成功（密钥残留）。
        let store = MockStore::failing_delete(Some(r#"{"openai":"sk-only"}"#));
        let result = delete_key_impl(&store, "openai");
        assert!(matches!(result, Err(AppError::Keychain(_))));
    }

    #[test]
    fn get_key_missing_provider_returns_none() {
        let store = MockStore::with_data(Some(r#"{"openai":"sk-a"}"#));
        assert_eq!(get_key_impl(&store, "deepseek").unwrap(), None);
        assert_eq!(
            get_key_impl(&store, "openai").unwrap().as_deref(),
            Some("sk-a")
        );
    }

    #[test]
    fn get_key_read_failure_propagates() {
        let store = MockStore::failing_get(Some(r#"{"openai":"sk-a"}"#));
        assert!(matches!(
            get_key_impl(&store, "openai"),
            Err(AppError::Keychain(_))
        ));
    }
}
