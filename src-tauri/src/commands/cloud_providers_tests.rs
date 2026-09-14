//! `cloud_providers` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests.rs`）。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use super::*;
use crate::db::Database;
use tempfile::NamedTempFile;

fn open_test_db() -> Database {
    let tmp = NamedTempFile::new().expect("临时文件创建失败");
    Database::open(tmp.path()).expect("数据库打开失败")
}

#[test]
fn validate_base_url_accepts_https_and_rejects_trailing_slash() {
    assert_eq!(
        validate_base_url("https://api.deepseek.com").unwrap(),
        "https://api.deepseek.com"
    );
    assert!(validate_base_url("https://api.example.com/v1").is_ok());
    assert!(validate_base_url("http://localhost:11434/v1").is_ok());
    assert!(validate_base_url("https://open.bigmodel.cn/api/paas/v4/").is_err()); // 尾斜杠
    assert!(validate_base_url("http://evil.com").is_err()); // 非本地 http
    assert!(validate_base_url("").is_err());
    assert!(validate_base_url("ftp://x").is_err());
}

#[test]
fn validate_website_allows_localhost_http_only() {
    assert!(validate_optional_website(Some("https://www.anthropic.com")).is_ok());
    assert!(validate_optional_website(Some("http://localhost:8080")).is_ok());
    assert!(validate_optional_website(Some("http://127.0.0.1:8080")).is_ok());
    assert!(validate_optional_website(Some("http://other.com")).is_err());
    assert!(validate_optional_website(Some("")).is_ok()); // 空当 None
    assert!(validate_optional_website(None).is_ok());
}

#[test]
fn list_seeded_returns_two_builtins() {
    let db = open_test_db();
    let list = ConfigRepo::list_cloud_providers(db.conn()).unwrap();
    assert_eq!(list.len(), 2, "迁移 V015 预置两条内建记录");
    let keys: Vec<&str> = list.iter().map(|r| r.provider_key.as_str()).collect();
    assert!(keys.contains(&"openai"));
    assert!(keys.contains(&"deepseek"));
}

#[test]
fn upsert_inserts_then_updates_same_key() {
    let db = open_test_db();
    let created = ConfigRepo::upsert_cloud_provider(
        db.conn(),
        "silicon-flow",
        "硅基流动",
        "国内代理",
        None,
        "https://api.siliconflow.cn",
    )
    .unwrap();
    assert_eq!(created.provider_key, "silicon-flow");
    assert_eq!(created.name, "硅基流动");
    assert!(!created.is_builtin);

    let updated = ConfigRepo::upsert_cloud_provider(
        db.conn(),
        "silicon-flow",
        "硅基流动 v2",
        "国内代理·高优先级",
        None,
        "https://api.siliconflow.cn/v1",
    )
    .unwrap();
    assert_eq!(updated.name, "硅基流动 v2");
    assert_eq!(updated.remark, "国内代理·高优先级");
    assert_eq!(updated.base_url, "https://api.siliconflow.cn/v1");
}

#[test]
fn delete_is_soft_idempotent_and_conflicts_on_reinsert() {
    let db = open_test_db();
    // 先新增一条自定义
    ConfigRepo::upsert_cloud_provider(
        db.conn(),
        "mars",
        "火星模型",
        "",
        None,
        "https://mars.example.com",
    )
    .unwrap();
    ConfigRepo::delete_cloud_provider(db.conn(), "mars").unwrap();
    // 第二次删幂等
    ConfigRepo::delete_cloud_provider(db.conn(), "mars").unwrap();
    // 列表只剩预置
    let list = ConfigRepo::list_cloud_providers(db.conn()).unwrap();
    assert!(list.iter().all(|r| r.provider_key != "mars"));
    // 同名再插报错
    let result = ConfigRepo::upsert_cloud_provider(
        db.conn(),
        "mars",
        "火星模型 复用",
        "",
        None,
        "https://mars.example.com",
    );
    assert!(result.is_err(), "软删除后的 key 再 upsert 应返回冲突错误");
}

#[test]
fn get_provider_base_url_matches_seeded() {
    let db = open_test_db();
    let deepseek = ConfigRepo::get_provider_base_url(db.conn(), "deepseek")
        .unwrap()
        .expect("预置 deepseek 必存在");
    assert_eq!(deepseek, "https://api.deepseek.com");
    let missing = ConfigRepo::get_provider_base_url(db.conn(), "unknown-xxx").unwrap();
    assert!(missing.is_none());
}
