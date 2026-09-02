//! API Key 安全存储：密钥只存系统 Keychain，以 JSON 单条目聚合。
//!
//! 安全规则（BE-C3）：聚合写回前必须成功读取存量密钥——读失败（如
//! Keychain 锁定、拒绝访问）时立即中止并返回错误，绝不能用空表兜底，
//! 否则一次写回会静默清空全部已存密钥。

use std::collections::HashMap;

use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "filemind";
const KEYRING_USER: &str = "api_keys";

/// debug 构建的 Keychain 影子文件（dev 兜底）。
///
/// macOS 开发模式下每次 `cargo build` 重编译会改变二进制 cdhash，Keychain 条目
/// 的 ACL 不再信任新二进制，读取 API Key 时报 errSecAuthFailed（"The user name
/// or passphrase you entered is not correct"）。本模块仅在 debug 构建编译：
/// 写 Keychain 时同步镜像一份聚合 JSON 到数据目录（Unix 下 0600 权限），读取被
/// 拒时回落到该文件，保证重编译后云端推理不断供。release 构建不编译本模块，
/// 密钥仍然只存 Keychain（安全红线 BE-C3 不受影响）。
#[cfg(debug_assertions)]
mod dev_shadow {
    use std::path::{Path, PathBuf};

    /// 影子文件名（位于数据目录根下）。
    const SHADOW_FILE_NAME: &str = "dev_api_keys.json";

    /// 解析数据目录：与 `main.rs` 的 `get_db_path` 同一规则
    /// （`FILEMIND_DATA_HOME` 优先，回退 `~/.filemind`）。
    fn data_home() -> PathBuf {
        if let Ok(dir) = std::env::var("FILEMIND_DATA_HOME") {
            return PathBuf::from(dir);
        }
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(std::env::temp_dir);
        home.join(".filemind")
    }

    /// 读取指定目录下的影子文件内容；不存在/为空/不可读返回 `None`。
    pub fn read_at(dir: &Path) -> Option<String> {
        std::fs::read_to_string(dir.join(SHADOW_FILE_NAME))
            .ok()
            .filter(|s| !s.trim().is_empty())
    }

    /// 写入指定目录下的影子文件（自动建目录，Unix 下收紧为 0600）。
    /// 影子文件是兜底通道，写入失败仅告警、不影响主流程。
    pub fn write_at(dir: &Path, value: &str) {
        let path = dir.join(SHADOW_FILE_NAME);
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("dev 影子密钥文件目录创建失败: {e}");
            return;
        }
        if let Err(e) = std::fs::write(&path, value) {
            log::warn!("dev 影子密钥文件写入失败: {e}");
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            {
                log::warn!("dev 影子密钥文件权限收紧失败: {e}");
            }
        }
    }

    /// 删除指定目录下的影子文件；不存在视为成功（幂等）。
    pub fn delete_at(dir: &Path) {
        if let Err(e) = std::fs::remove_file(dir.join(SHADOW_FILE_NAME)) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("dev 影子密钥文件删除失败: {e}");
            }
        }
    }

    /// 读取当前数据目录下的影子文件内容；不存在返回 `None`。
    pub fn read_current() -> Option<String> {
        read_at(&data_home())
    }

    /// 写入当前数据目录下的影子文件。
    pub fn write_current(value: &str) {
        write_at(&data_home(), value);
    }

    /// 删除当前数据目录下的影子文件（幂等）。
    pub fn delete_current() {
        delete_at(&data_home());
    }
}

/// Keychain 访问类失败（errSecAuthFailed 等）的统一错误信息：附加可操作指引。
/// release 构建无影子文件兜底，用户需自助修复（重存 Key 或重建条目）。
fn access_error_with_hint(context: &str, e: &keyring::Error) -> AppError {
    /// 访问失败的可操作指引（重存 Key / 重建 Keychain 条目）。
    const HINT: &str = "（疑似钥匙串拒绝当前应用访问，macOS 开发模式重编译后常见；\
        可在设置页重新保存 API Key，或运行 \
        security add-generic-password -A -s filemind -a api_keys -w <密钥> 重建条目）";
    AppError::Keychain(format!("{context}: {e}{HINT}"))
}

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
            // 访问类失败（errSecAuthFailed / 钥匙串锁定）：
            // debug 构建回落影子文件；release（或无影子文件）返回带指引的错误。
            Err(e @ (keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_))) => {
                #[cfg(debug_assertions)]
                if let Some(data) = dev_shadow::read_current() {
                    log::warn!("Keychain 访问被拒（{e}），debug 构建回落到影子文件");
                    return Ok(Some(data));
                }
                Err(access_error_with_hint("Keychain 读取失败", &e))
            }
            Err(e) => Err(AppError::Keychain(format!("Keychain 读取失败: {e}"))),
        }
    }

    fn set(&self, value: &str) -> AppResult<()> {
        match self.entry.set_password(value) {
            Ok(()) => {
                // debug 构建同步镜像到影子文件，供重编译后 Keychain 失信时兜底。
                #[cfg(debug_assertions)]
                dev_shadow::write_current(value);
                Ok(())
            }
            // debug 构建下写入失败（多为 ACL 拒绝）：退化为仅写影子文件，
            // 保住用户的保存操作；下次 Keychain 恢复信任时自然回归主存储。
            #[cfg(debug_assertions)]
            Err(e) => {
                dev_shadow::write_current(value);
                log::warn!("Keychain 写入失败（{e}），debug 构建退化为仅写影子文件");
                Ok(())
            }
            #[cfg(not(debug_assertions))]
            Err(e) => Err(AppError::Keychain(format!("Keychain 存储失败: {e}"))),
        }
    }

    fn delete(&self) -> AppResult<bool> {
        match self.entry.delete_credential() {
            Ok(()) => {
                // 条目已删：同步清理影子文件，避免残留已删除的密钥。
                #[cfg(debug_assertions)]
                dev_shadow::delete_current();
                Ok(true)
            }
            // 条目本就不存在：视为幂等删除成功。
            Err(keyring::Error::NoEntry) => Ok(false),
            // debug 构建下条目删除失败（同样多为 ACL 拒绝）：仅清理影子文件并
            // 视为成功，避免开发模式删除 Key 一直报错；残留条目以 warn 留痕。
            #[cfg(debug_assertions)]
            Err(e) => {
                log::warn!("Keychain 删除失败（{e}），debug 构建仅清理影子文件");
                dev_shadow::delete_current();
                Ok(true)
            }
            // release 构建下删除失败照常报错（BE-m2：残留密钥不得静默成功）。
            #[cfg(not(debug_assertions))]
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

    // ==== dev_shadow 影子文件（debug 兜底）测试 ====

    /// 构造测试专用临时目录（进程隔离，避免并发测试互踩）。
    fn shadow_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "filemind-shadow-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    #[cfg(debug_assertions)]
    fn dev_shadow_write_read_delete_roundtrip() {
        let dir = shadow_test_dir("roundtrip");
        let payload = r#"{"openai":"sk-a","deepseek":"sk-b"}"#;
        super::dev_shadow::write_at(&dir, payload);
        assert_eq!(
            super::dev_shadow::read_at(&dir).as_deref(),
            Some(payload),
            "写入后应能读回相同内容"
        );
        super::dev_shadow::delete_at(&dir);
        assert_eq!(
            super::dev_shadow::read_at(&dir),
            None,
            "删除后应读不到任何内容"
        );
        // 幂等删除：再次删除不报错。
        super::dev_shadow::delete_at(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn dev_shadow_read_missing_dir_returns_none() {
        let dir = shadow_test_dir("missing");
        assert_eq!(
            super::dev_shadow::read_at(&dir),
            None,
            "目录不存在时返回 None"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn dev_shadow_write_ignores_empty_content_on_read() {
        let dir = shadow_test_dir("empty");
        super::dev_shadow::write_at(&dir, "");
        assert_eq!(
            super::dev_shadow::read_at(&dir),
            None,
            "空内容视为无影子数据"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(debug_assertions, unix))]
    fn dev_shadow_file_permissions_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = shadow_test_dir("perm");
        super::dev_shadow::write_at(&dir, r#"{"openai":"sk-a"}"#);
        let mode = std::fs::metadata(dir.join("dev_api_keys.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "影子文件必须仅属主可读写（明文密钥）");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
