//! 分类查询：扁平列表与树形结构（原 `category_repo.rs` 拆出）。

use std::collections::HashMap;

use rusqlite::Connection;

use super::{CategoryRepo, SELECT_ALL_SQL};
use crate::db::models::{Category, CategoryNode};
use crate::error::AppResult;

impl CategoryRepo {
    /// 查询全部分类（扁平列表，按 `sort_order` 升序）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_all(conn: &Connection) -> AppResult<Vec<Category>> {
        let mut stmt = conn.prepare(SELECT_ALL_SQL)?;
        let rows = stmt.query_map([], super::map_category)?;
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
}
