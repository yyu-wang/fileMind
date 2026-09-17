//! `category_repo` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `category_repo/mod.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。
//! 注意 `super` 指 `db::category_repo` 模块，不是文件所在目录 `db`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::db::category_repo::seed::BUILTIN_CATEGORIES;
use crate::db::database::Database;
use crate::error::AppError;
use tempfile::NamedTempFile;

fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    Ok(Database::open(tmp.path())?)
}

fn mk_category(id: &str, name: &str, parent: Option<&str>, builtin: bool) -> Category {
    Category {
        id: id.to_string(),
        name: name.to_string(),
        parent_id: parent.map(std::string::ToString::to_string),
        icon: None,
        color: None,
        sort_order: 0,
        is_builtin: builtin,
        target_dir: String::new(),
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    }
}

#[test]
fn test_seed_builtin_categories_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;

    let first = CategoryRepo::seed_builtin_categories(db.conn())?;
    assert_eq!(first, BUILTIN_CATEGORIES.len(), "首次应全部新增");

    // 二次调用幂等：不重复插入
    let second = CategoryRepo::seed_builtin_categories(db.conn())?;
    assert_eq!(second, 0, "二次调用不应新增");

    let all = CategoryRepo::list_all(db.conn())?;
    assert_eq!(all.len(), BUILTIN_CATEGORIES.len());

    // target_dir 已填充（与 name 一致），供分类器启发式使用
    let image = all
        .iter()
        .find(|c| c.id == "builtin-image")
        .ok_or("内置图片分类应存在")?;
    assert_eq!(image.name, "图片");
    assert_eq!(image.target_dir, "图片");
    assert!(image.is_builtin);
    Ok(())
}

#[test]
fn test_upsert_persists_target_dir() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let mut c1 = mk_category("c1", "自定义", None, false);
    c1.target_dir = "my/sub".to_string();
    CategoryRepo::upsert(db.conn(), &c1)?;

    let all = CategoryRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].target_dir, "my/sub");
    Ok(())
}

#[test]
fn test_upsert_and_list_all() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let c1 = mk_category("c1", "市场", None, true);
    let c2 = mk_category("c2", "竞品", Some("c1"), false);
    CategoryRepo::upsert(db.conn(), &c1)?;
    CategoryRepo::upsert(db.conn(), &c2)?;

    let all = CategoryRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 2);
    // sort_order 都为 0 → 按 name 升序（SQLite 默认 BINARY 排序，中文按 UTF-8 字节序）
    // 不假设中文拼音顺序，只验证两条都在
    let ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"c1"));
    assert!(ids.contains(&"c2"));
    Ok(())
}

#[test]
fn test_upsert_overwrite() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let mut c1 = mk_category("c1", "市场", None, false);
    CategoryRepo::upsert(db.conn(), &c1)?;

    // 二次调用改名
    c1.name = "市场部".to_string();
    CategoryRepo::upsert(db.conn(), &c1)?;

    let all = CategoryRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "市场部");
    Ok(())
}

#[test]
fn test_list_tree_structure() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    // 树结构：
    //   c1 (根)
    //     c2 (子)
    //       c3 (孙)
    //   c4 (根)
    CategoryRepo::upsert(db.conn(), &mk_category("c1", "市场", None, true))?;
    CategoryRepo::upsert(db.conn(), &mk_category("c2", "竞品", Some("c1"), false))?;
    CategoryRepo::upsert(db.conn(), &mk_category("c3", "抖音", Some("c2"), false))?;
    CategoryRepo::upsert(db.conn(), &mk_category("c4", "产品", None, false))?;

    let tree = CategoryRepo::list_tree(db.conn())?;
    // 2 个根节点
    assert_eq!(tree.len(), 2);

    let c1_node = tree.iter().find(|n| n.category.id == "c1").unwrap();
    assert_eq!(c1_node.children.len(), 1);

    let c2_node = &c1_node.children[0];
    assert_eq!(c2_node.category.id, "c2");
    assert_eq!(c2_node.children.len(), 1);

    let c3_node = &c2_node.children[0];
    assert_eq!(c3_node.category.id, "c3");
    assert!(c3_node.children.is_empty());

    let c4_node = tree.iter().find(|n| n.category.id == "c4").unwrap();
    assert!(c4_node.children.is_empty());
    Ok(())
}

#[test]
fn test_delete_user_category() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    CategoryRepo::upsert(db.conn(), &mk_category("c1", "用户分类", None, false))?;
    CategoryRepo::delete(db.conn(), "c1")?;

    let all = CategoryRepo::list_all(db.conn())?;
    assert!(all.is_empty());
    Ok(())
}

#[test]
fn test_delete_builtin_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    CategoryRepo::upsert(db.conn(), &mk_category("c1", "系统分类", None, true))?;

    let r = CategoryRepo::delete(db.conn(), "c1");
    assert!(matches!(r, Err(AppError::Forbidden(_))));

    // 数据还在
    let all = CategoryRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 1);
    Ok(())
}

#[test]
fn test_delete_nonexistent_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let r = CategoryRepo::delete(db.conn(), "nonexistent");
    assert!(r.is_err());
    Ok(())
}

#[test]
fn test_list_tree_empty() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let tree = CategoryRepo::list_tree(db.conn())?;
    assert!(tree.is_empty());
    Ok(())
}

#[test]
fn test_sort_order_applied() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let mut c1 = mk_category("c1", "高优先级", None, false);
    c1.sort_order = 100;
    let mut c2 = mk_category("c2", "低优先级", None, false);
    c2.sort_order = 1;
    CategoryRepo::upsert(db.conn(), &c1)?;
    CategoryRepo::upsert(db.conn(), &c2)?;

    let all = CategoryRepo::list_all(db.conn())?;
    // sort_order 升序 → c2 在前
    assert_eq!(all[0].id, "c2");
    assert_eq!(all[1].id, "c1");
    Ok(())
}
