//! `categories` 表数据仓库：扁平/树形查询、upsert 与删除（内置保护）。

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::db::models::{Category, CategoryNode};
use crate::error::{AppError, AppResult};

const UPSERT_CATEGORY_SQL: &str = "
    INSERT INTO categories (id, name, parent_id, icon, color, sort_order, is_builtin)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
    ON CONFLICT(id) DO UPDATE SET
        name = excluded.name,
        parent_id = excluded.parent_id,
        icon = excluded.icon,
        color = excluded.color,
        sort_order = excluded.sort_order,
        is_builtin = excluded.is_builtin,
        updated_at = datetime('now')
";

const SELECT_ALL_SQL: &str = "
    SELECT id, name, parent_id, icon, color, sort_order, is_builtin, created_at, updated_at
    FROM categories
    ORDER BY sort_order ASC, name ASC
";

/// `categories` 表仓库。
pub struct CategoryRepo;

impl CategoryRepo {
    /// 查询全部分类（扁平列表，按 `sort_order` 升序）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_all(conn: &Connection) -> AppResult<Vec<Category>> {
        let mut stmt = conn.prepare(SELECT_ALL_SQL)?;
        let rows = stmt.query_map([], map_category)?;
        let mut categories = Vec::new();
        for row in rows {
            categories.push(row?);
        }
        Ok(categories)
    }

    /// 查询分类树：在 Rust 内把扁平列表组装为树结构，避免 SQL 递归 CTE。
    ///
    /// # Errors
    ///
    /// 底层 `list_all` 失败时返回错误。
    pub fn list_tree(conn: &Connection) -> AppResult<Vec<CategoryNode>> {
        let flat = Self::list_all(conn)?;
        Ok(Self::build_tree(flat))
    }

    /// 把扁平 `Category` 列表组装成树。
    /// 单独提出来便于复用 + 测试，避免与 SQL 查询耦合。
    fn build_tree(flat: Vec<Category>) -> Vec<CategoryNode> {
        // 按 parent_id 分组：parent_id 为 None 的进 roots
        // 借用 flat，避免克隆整个 Category
        let mut children_by_parent: HashMap<Option<&str>, Vec<&Category>> = HashMap::new();
        for cat in &flat {
            children_by_parent
                .entry(cat.parent_id.as_deref())
                .or_default()
                .push(cat);
        }
        Self::build_subtree(None, &children_by_parent)
    }

    fn build_subtree(
        parent: Option<&str>,
        children_by_parent: &HashMap<Option<&str>, Vec<&Category>>,
    ) -> Vec<CategoryNode> {
        children_by_parent
            .get(&parent)
            .map(|cats| {
                cats.iter()
                    .map(|cat| CategoryNode {
                        category: (*cat).clone(),
                        children: Self::build_subtree(Some(cat.id.as_str()), children_by_parent),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 新增或更新分类（INSERT OR REPLACE，按 id 冲突更新）。
    ///
    /// # Errors
    ///
    /// 写入失败时返回数据库错误。
    pub fn upsert(conn: &Connection, category: &Category) -> AppResult<()> {
        conn.execute(
            UPSERT_CATEGORY_SQL,
            params![
                category.id,
                category.name,
                category.parent_id,
                category.icon,
                category.color,
                category.sort_order,
                category.is_builtin,
            ],
        )?;
        Ok(())
    }

    /// 删除分类。
    ///
    /// 安全规则：``is_builtin=1`` 的系统预置分类拒绝删除（返回 `Forbidden`）。
    /// 删除前需先查 `is_builtin`，避免误删。
    ///
    /// # Errors
    ///
    /// - 分类不存在返回 `QueryReturnedNoRows`
    /// - 内置分类返回 `Forbidden`
    /// - 删除失败返回数据库错误
    pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
        let is_builtin: i64 = conn.query_row(
            "SELECT is_builtin FROM categories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        if is_builtin == 1 {
            return Err(AppError::Forbidden(format!(
                "系统预置分类不可删除 (id={id})"
            )));
        }

        let affected = conn.execute(
            "DELETE FROM categories WHERE id = ?1 AND is_builtin = 0",
            params![id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }
}

/// 将查询行映射为 [`Category`]。
fn map_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        parent_id: row.get(2)?,
        icon: row.get(3)?,
        color: row.get(4)?,
        sort_order: row.get(5)?,
        is_builtin: row.get::<_, i64>(6)? != 0,
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

    fn mk_category(id: &str, name: &str, parent: Option<&str>, builtin: bool) -> Category {
        Category {
            id: id.to_string(),
            name: name.to_string(),
            parent_id: parent.map(|p| p.to_string()),
            icon: None,
            color: None,
            sort_order: 0,
            is_builtin: builtin,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
        }
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
}
