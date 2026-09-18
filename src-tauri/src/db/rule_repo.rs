//! `rules` 表数据仓库：按优先级查询、upsert 与删除。
//!
//! 单元测试在兄弟文件 `rule_repo_tests.rs`（原内嵌，与实现合计超 Rust 模块
//! 300 行警告阈值），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。

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

/// 内置默认规则种子：开箱即用的规则模板，默认禁用（`is_enabled=0`），用户一键启用。
///
/// `id` 用稳定 slug 保证幂等；`pattern` 与分类器 `extension` 语义一致（逗号分隔扩展名）。
struct DefaultRuleSeed {
    id: &'static str,
    name: &'static str,
    pattern: &'static str,
}

const DEFAULT_RULES: &[DefaultRuleSeed] = &[
    DefaultRuleSeed {
        id: "default_rule_pdf",
        name: "PDF 文件归档",
        pattern: "pdf",
    },
    DefaultRuleSeed {
        id: "default_rule_text",
        name: "文本文件归档",
        pattern: "txt,md",
    },
];

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

    /// 幂等写入内置默认规则（默认禁用，供用户直接勾选启用）。
    ///
    /// 仅当 `rules` 表为空时执行（首次启动或用户删光全部规则后的回退态），避免把
    /// 用户已删除的默认规则反复加回、干扰已有自定义规则。目标分类固定指向内置分类
    /// `builtin-document`，调用方需先执行 `CategoryRepo::seed_builtin_categories`
    /// 保证外键引用有效。返回实际新增条数。
    ///
    /// # Errors
    ///
    /// 数量查询或写入失败时返回数据库错误。
    pub fn seed_default_rules(conn: &Connection) -> AppResult<usize> {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM rules", [], |row| row.get(0))?;
        if count > 0 {
            return Ok(0);
        }
        let mut inserted = 0usize;
        for seed in DEFAULT_RULES {
            let affected = conn.execute(
                "INSERT INTO rules (id, name, rule_type, pattern, target_category, priority, is_enabled)
                 VALUES (?1, ?2, 'extension', ?3, 'builtin-document', 40, 0)",
                params![seed.id, seed.name, seed.pattern],
            )?;
            inserted += affected;
        }
        Ok(inserted)
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
#[path = "rule_repo_tests.rs"]
mod tests;
