//! `rule_repo` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `rule_repo.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。

use super::*;
use crate::db::category_repo::CategoryRepo;
use crate::db::database::Database;
use tempfile::NamedTempFile;

fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    Ok(Database::open(tmp.path())?)
}

/// 补齐内置分类种子（默认规则外键指向 builtin-document）。
fn seed_categories(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    CategoryRepo::seed_builtin_categories(conn)?;
    Ok(())
}

fn mk_rule(id: &str, name: &str, priority: i64, enabled: bool) -> Rule {
    Rule {
        id: id.to_string(),
        name: name.to_string(),
        rule_type: "extension".to_string(),
        pattern: "pdf,doc".to_string(),
        // 用 None 避免 rules.target_category 外键约束失败（categories 表无对应分类）
        target_category: None,
        priority,
        is_enabled: enabled,
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    }
}

#[test]
fn test_upsert_and_list_all() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let r1 = mk_rule("r1", "PDF 规则", 100, true);
    let r2 = mk_rule("r2", "Word 规则", 50, false);
    RuleRepo::upsert(db.conn(), &r1)?;
    RuleRepo::upsert(db.conn(), &r2)?;

    let all = RuleRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 2);
    // priority DESC → r1 (100) 在前
    assert_eq!(all[0].id, "r1");
    assert_eq!(all[1].id, "r2");
    Ok(())
}

#[test]
fn test_list_enabled_filters_disabled() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let r1 = mk_rule("r1", "启用规则", 100, true);
    let r2 = mk_rule("r2", "禁用规则", 200, false);
    RuleRepo::upsert(db.conn(), &r1)?;
    RuleRepo::upsert(db.conn(), &r2)?;

    let enabled = RuleRepo::list_enabled(db.conn())?;
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].id, "r1");
    Ok(())
}

#[test]
fn test_list_enabled_priority_desc() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    // 三个启用规则，优先级不同
    RuleRepo::upsert(db.conn(), &mk_rule("r1", "低", 10, true))?;
    RuleRepo::upsert(db.conn(), &mk_rule("r2", "高", 100, true))?;
    RuleRepo::upsert(db.conn(), &mk_rule("r3", "中", 50, true))?;

    let enabled = RuleRepo::list_enabled(db.conn())?;
    assert_eq!(enabled.len(), 3);
    // priority DESC → r2 (100), r3 (50), r1 (10)
    assert_eq!(enabled[0].id, "r2");
    assert_eq!(enabled[1].id, "r3");
    assert_eq!(enabled[2].id, "r1");
    Ok(())
}

#[test]
fn test_upsert_overwrite() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let mut r1 = mk_rule("r1", "原名", 100, true);
    RuleRepo::upsert(db.conn(), &r1)?;

    // 二次调用改名 + 禁用
    r1.name = "新名".to_string();
    r1.is_enabled = false;
    RuleRepo::upsert(db.conn(), &r1)?;

    let all = RuleRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "新名");
    assert!(!all[0].is_enabled);

    // 启用规则列表应该为空
    let enabled = RuleRepo::list_enabled(db.conn())?;
    assert!(enabled.is_empty());
    Ok(())
}

#[test]
fn test_delete() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    RuleRepo::upsert(db.conn(), &mk_rule("r1", "规则", 100, true))?;

    RuleRepo::delete(db.conn(), "r1")?;
    let all = RuleRepo::list_all(db.conn())?;
    assert!(all.is_empty());
    Ok(())
}

#[test]
fn test_delete_nonexistent_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let r = RuleRepo::delete(db.conn(), "nonexistent");
    assert!(r.is_err());
    Ok(())
}

#[test]
fn test_list_enabled_empty() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let enabled = RuleRepo::list_enabled(db.conn())?;
    assert!(enabled.is_empty());
    Ok(())
}

#[test]
fn test_priority_same_name_asc_tiebreaker() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    // 同优先级 → 按 name 升序
    RuleRepo::upsert(db.conn(), &mk_rule("r1", "Zeta", 100, true))?;
    RuleRepo::upsert(db.conn(), &mk_rule("r2", "Alpha", 100, true))?;

    let enabled = RuleRepo::list_enabled(db.conn())?;
    assert_eq!(enabled[0].id, "r2"); // Alpha < Zeta
    assert_eq!(enabled[1].id, "r1");
    Ok(())
}

#[test]
fn test_reorder_assigns_priority_by_order() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    RuleRepo::upsert(db.conn(), &mk_rule("r1", "一", 100, true))?;
    RuleRepo::upsert(db.conn(), &mk_rule("r2", "二", 50, true))?;
    RuleRepo::upsert(db.conn(), &mk_rule("r3", "三", 10, true))?;

    // 拖拽后最终顺序：r3 最优先，r1 次之，r2 最后
    let ordered = ["r3".to_string(), "r1".to_string(), "r2".to_string()];
    RuleRepo::reorder(db.conn(), &ordered)?;

    // priority 分配：r3=3, r1=2, r2=1 → 列表顺序 r3, r1, r2
    let all = RuleRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, "r3");
    assert_eq!(all[0].priority, 3);
    assert_eq!(all[1].id, "r1");
    assert_eq!(all[1].priority, 2);
    assert_eq!(all[2].id, "r2");
    assert_eq!(all[2].priority, 1);
    Ok(())
}

#[test]
fn test_reorder_empty_list_is_noop() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let empty: Vec<String> = Vec::new();
    RuleRepo::reorder(db.conn(), &empty)?;
    assert!(RuleRepo::list_all(db.conn())?.is_empty());
    Ok(())
}

#[test]
fn test_seed_default_rules_inserts_disabled_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    seed_categories(db.conn())?;

    let inserted = RuleRepo::seed_default_rules(db.conn())?;
    assert_eq!(inserted, DEFAULT_RULES.len());

    let all = RuleRepo::list_all(db.conn())?;
    assert_eq!(all.len(), DEFAULT_RULES.len());
    for rule in &all {
        // 默认不勾选：全部禁用
        assert!(!rule.is_enabled);
        assert_eq!(rule.rule_type, "extension");
        assert_eq!(rule.priority, 40);
        // 目标分类指向内置「文档」，保证外键与展示一致
        assert_eq!(rule.target_category.as_deref(), Some("builtin-document"));
    }
    let pdf = all
        .iter()
        .find(|r| r.id == "default_rule_pdf")
        .ok_or("缺少 PDF 默认规则")?;
    assert_eq!(pdf.name, "PDF 文件归档");
    assert_eq!(pdf.pattern, "pdf");
    let text = all
        .iter()
        .find(|r| r.id == "default_rule_text")
        .ok_or("缺少文本默认规则")?;
    assert_eq!(text.name, "文本文件归档");
    assert_eq!(text.pattern, "txt,md");
    Ok(())
}

#[test]
fn test_seed_default_rules_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    seed_categories(db.conn())?;

    let first = RuleRepo::seed_default_rules(db.conn())?;
    let second = RuleRepo::seed_default_rules(db.conn())?;
    assert_eq!(first, DEFAULT_RULES.len());
    assert_eq!(second, 0);
    assert_eq!(RuleRepo::list_all(db.conn())?.len(), DEFAULT_RULES.len());
    Ok(())
}

#[test]
fn test_seed_default_rules_skips_when_user_rules_exist() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    seed_categories(db.conn())?;
    // 已有用户自定义规则 → 不再注入默认规则
    RuleRepo::upsert(db.conn(), &mk_rule("r1", "我的规则", 100, true))?;

    let inserted = RuleRepo::seed_default_rules(db.conn())?;
    assert_eq!(inserted, 0);

    let all = RuleRepo::list_all(db.conn())?;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "r1");
    Ok(())
}
