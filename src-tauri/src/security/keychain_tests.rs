//! `keychain` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests/` 目录）。

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
