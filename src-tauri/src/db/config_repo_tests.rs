//! `config_repo` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `config_repo.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/rule_repo_tests.rs`）。

// 测试代码允许 unwrap/expect/panic：简洁直观地表达失败语义
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use super::*;
use crate::db::Database;
use tempfile::NamedTempFile;

/// 构建临时文件数据库并跑全部迁移。
fn open_test_db() -> Database {
    let tmp = NamedTempFile::new().expect("临时文件创建失败");
    Database::open(tmp.path()).expect("数据库打开失败")
}

#[test]
fn get_returns_default_after_migration() {
    let db = open_test_db();
    let config = ConfigRepo::get(db.conn()).expect("读取默认配置");
    assert_eq!(config.inference_mode, "local");
    assert_eq!(config.llm_model, "qwen3.8-27b");
    assert!(!config.onboarding_completed);
    assert!(!config.cloud_consent_signed);
    assert!(config.cloud_consent_version.is_none());
    assert!(config.cloud_model.is_empty());
}

#[test]
fn upsert_persists_all_fields() {
    let db = open_test_db();
    let mut config = ConfigRepo::get(db.conn()).unwrap();
    config.data_directory = "/tmp/test".to_string();
    config.inference_mode = "cloud".to_string();
    config.llm_model = "qwen3.8-14b".to_string();
    config.cloud_model = "gpt-4o".to_string();
    config.onboarding_completed = true;
    config.max_file_size_mb = 200;
    ConfigRepo::upsert(db.conn(), &config).unwrap();

    let reloaded = ConfigRepo::get(db.conn()).unwrap();
    assert_eq!(reloaded.data_directory, "/tmp/test");
    assert_eq!(reloaded.inference_mode, "cloud");
    assert_eq!(reloaded.llm_model, "qwen3.8-14b");
    assert_eq!(reloaded.cloud_model, "gpt-4o");
    assert!(reloaded.onboarding_completed);
    assert_eq!(reloaded.max_file_size_mb, 200);
}

#[test]
fn sign_consent_sets_fields_and_switches_mode() {
    let db = open_test_db();
    ConfigRepo::sign_consent(db.conn(), "v1.0", "openai".to_string()).unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    assert!(config.cloud_consent_signed);
    assert_eq!(config.cloud_consent_version.as_deref(), Some("v1.0"));
    assert_eq!(config.inference_mode, "cloud");
}

#[test]
fn revoke_consent_clears_fields_and_switches_back() {
    let db = open_test_db();
    ConfigRepo::sign_consent(db.conn(), "v1.0", "deepseek".to_string()).unwrap();
    ConfigRepo::revoke_consent(db.conn()).unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    assert!(!config.cloud_consent_signed);
    assert!(config.cloud_consent_version.is_none());
    assert_eq!(config.inference_mode, "local");
}

#[test]
fn provider_roundtrip_preserves_value() {
    let db = open_test_db();
    ConfigRepo::sign_consent(db.conn(), "v1.0", "deepseek".to_string()).unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    match config.cloud_consent_provider {
        Some(ref v) if v == "deepseek" => {}
        other => panic!("期望 deepseek，实际 {other:?}"),
    }
}

/// BE-M2：签署时间必须是合法 RFC3339（前端 `new Date()` 可解析）。
#[test]
fn sign_consent_writes_rfc3339_timestamp() {
    let db = open_test_db();
    ConfigRepo::sign_consent(db.conn(), "v1.0", "openai".to_string()).unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    let signed_at = config.cloud_consent_signed_at.expect("签署后必有时间");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&signed_at).is_ok(),
        "signed_at 应为 RFC3339，实际: {signed_at}"
    );
    assert!(signed_at.ends_with('Z'), "应为 UTC（Z 结尾）: {signed_at}");
}

#[test]
fn active_cloud_provider_roundtrips_through_upsert() {
    let db = open_test_db();
    let mut config = ConfigRepo::get(db.conn()).unwrap();
    assert!(config.active_cloud_provider.is_none());
    config.active_cloud_provider = Some("my-custom".to_string());
    ConfigRepo::upsert(db.conn(), &config).unwrap();
    let reloaded = ConfigRepo::get(db.conn()).unwrap();
    assert_eq!(reloaded.active_cloud_provider.as_deref(), Some("my-custom"));
}

/// V019：迁移后旧库默认「ollama + 内置 GGUF 标识」——既有用户推理路径不变。
#[test]
fn local_llm_defaults_after_migration() {
    let db = open_test_db();
    let config = ConfigRepo::get(db.conn()).expect("读取默认配置");
    assert_eq!(config.local_llm_backend, "ollama");
    assert_eq!(config.local_llm_model, "qwen2.5-3b-instruct");
}

/// 切到内置后端后经 upsert 往返不丢值（T3 据此决定是否启用内置 llama.cpp）。
#[test]
fn local_llm_backend_roundtrips_through_upsert() {
    let db = open_test_db();
    let mut config = ConfigRepo::get(db.conn()).unwrap();
    config.local_llm_backend = "builtin".to_string();
    ConfigRepo::upsert(db.conn(), &config).unwrap();

    let reloaded = ConfigRepo::get(db.conn()).unwrap();
    assert_eq!(reloaded.local_llm_backend, "builtin");
    assert_eq!(reloaded.local_llm_model, "qwen2.5-3b-instruct");
}

/// BE-M2 兼容：旧版 `epoch:` 前缀值读取时归一化为 RFC3339，语义不变。
#[test]
fn legacy_epoch_signed_at_normalized_on_read() {
    let db = open_test_db();
    // 直接预置旧版数据（模拟升级前已签署用户），绕过 sign_consent 的新写入路径
    db.conn()
        .execute(
            "UPDATE app_config SET cloud_consent_signed = 1, cloud_consent_signed_at = 'epoch:1756000000' WHERE id = 1",
            [],
        )
        .unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    let signed_at = config.cloud_consent_signed_at.expect("旧值不应丢失");
    let dt = chrono::DateTime::parse_from_rfc3339(&signed_at).expect("读取时应已转为 RFC3339");
    assert_eq!(dt.timestamp(), 1_756_000_000, "时间语义不应改变");
}

/// BE-M2 兜底：非法 epoch 值（非数字）读取时原样保留，不 panic。
#[test]
fn malformed_epoch_value_preserved_as_is() {
    let db = open_test_db();
    db.conn()
        .execute(
            "UPDATE app_config SET cloud_consent_signed_at = 'epoch:not-a-number' WHERE id = 1",
            [],
        )
        .unwrap();
    let config = ConfigRepo::get(db.conn()).unwrap();
    assert_eq!(
        config.cloud_consent_signed_at.as_deref(),
        Some("epoch:not-a-number")
    );
}
