//! `rules` 表数据仓库：按优先级查询、upsert 与删除。

use rusqlite::{params, Connection};

use crate::db::models::Rule;
use crate::error::AppResult;

const UPSERT_RULE_SQL: &str = "
    INSERT INTO rules (id, name, rule_type, pattern, target_category, priority, is_enabled)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
    ON CONFLICT(id) DO UPDATE SET
        name = excluded.name,
        rule_type = excluded.rule_type,
        pattern = excluded.pattern,
        target_category = excluded.target_category,
        priority = excluded.priority,
        is_enabled = excluded.is_enabled,
        updated_at = datetime('now')
";

const SELECT_ENABLED_SQL: &str = "
    SELECT id, name, rule_type, pattern, target_category, priority, is_enabled, created_at, updated_at
    FROM rules
    WHERE is_enabled = 1
    ORDER BY priority DESC, name ASC
";

const SELECT_ALL_SQL: &str = "
    SELECT id, name, rule_type, pattern, target_category, priority, is_enabled, created_at, updated_at
    FROM rules
    ORDER BY priority DESC, name ASC
";

/// `rules` 表仓库。
pub struct RuleRepo;

impl RuleRepo {
    /// 查询所有启用的规则，按优先级降序（数字越大越先匹配）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_enabled(conn: &Connection) -> AppResult<Vec<Rule>> {
        let mut stmt = conn.prepare(SELECT_ENABLED_SQL)?;
        let rows = stmt.query_map([], map_rule)?;
        let mut rules = Vec::new();
        for row in rows {
            rules.push(row?);
        }
        Ok(rules)
    }

    /// 查询全部规则（含禁用项），规则编辑页用。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_all(conn: &Connection) -> AppResult<Vec<Rule>> {
        let mut stmt = conn.prepare(SELECT_ALL_SQL)?;
        let rows = stmt.query_map([], map_rule)?;
        let mut rules = Vec::new();
        for row in rows {
            rules.push(row?);
        }
        Ok(rules)
    }

    /// 按 ID 查询单条规则。
    ///
    /// # Errors
    ///
    /// 规则不存在时返回 `QueryReturnedNoRows`；查询失败返回数据库错误。
    pub fn get_by_id(conn: &Connection, id: &str) -> AppResult<Rule> {
        conn.query_row(
            "SELECT id, name, rule_type, pattern, target_category, priority, is_enabled, created_at, updated_at \
             FROM rules WHERE id = ?1",
            params![id],
            map_rule,
        )
        .map_err(Into::into)
    }

    /// 新增或更新规则（id 冲突时更新）。
    ///
    /// # Errors
    ///
    /// 写入失败时返回数据库错误。
    pub fn upsert(conn: &Connection, rule: &Rule) -> AppResult<()> {
        conn.execute(
            UPSERT_RULE_SQL,
            params![
                rule.id,
                rule.name,
                rule.rule_type,
                rule.pattern,
                rule.target_category,
                rule.priority,
                rule.is_enabled,
            ],
        )?;
        Ok(())
    }

    /// 删除规则。
    ///
    /// # Errors
    ///
    /// 规则不存在时返回 `QueryReturnedNoRows`；删除失败返回数据库错误。
    pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
        let affected = conn.execute("DELETE FROM rules WHERE id = ?1", params![id])?;

        if affected == 0 {
            return Err(crate::error::AppError::Database(
                rusqlite::Error::QueryReturnedNoRows,
            ));
        }
        Ok(())
    }

    /// 按给定顺序批量重排规则优先级（拖拽排序用）。
    ///
    /// `ordered_ids` 为最终展示顺序（index 0 最优先），按 `priority = len - i`
    /// 逆序分配（数字越大越先匹配，符合 `ORDER BY priority DESC`）。
    ///
    /// # Errors
    ///
    /// 事务开启、更新或提交失败时返回错误。
    pub fn reorder(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
        let tx = conn.unchecked_transaction()?;
        // 规则数量极小（<100），usize→i64 不会溢出；unwrap_or 兜底空表
        let len = i64::try_from(ordered_ids.len()).unwrap_or(0);
        for (i, id) in ordered_ids.iter().enumerate() {
            let priority = len - i64::try_from(i).unwrap_or(0);
            tx.execute(
                "UPDATE rules SET priority = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![priority, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// 将查询行映射为 [`Rule`]。
fn map_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<Rule> {
    Ok(Rule {
        id: row.get(0)?,
        name: row.get(1)?,
        rule_type: row.get(2)?,
        pattern: row.get(3)?,
        target_category: row.get(4)?,
        priority: row.get(5)?,
        is_enabled: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::database::Database;
    use tempfile::NamedTempFile;

    fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        Ok(Database::open(tmp.path())?)
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
}
