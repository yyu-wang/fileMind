//! `categories` 表数据仓库：扁平/树形查询、upsert 与删除（内置保护）。
//!
//! 模块划分（原单文件 486 行，逼近 Rust 模块 500 行强制阈值，见 `rules/complexity.md`）：
//!   - `seed` 内置分类种子与幂等写入（`is_builtin=1`，启发式兜底的目标分类）
//!   - `list` 扁平列表与树形查询（树在 Rust 内组装，避免 SQL 递归 CTE）
//!   - `crud` 新增/更新与删除（内置分类禁删）
//!
//! 各文件中的 `impl CategoryRepo` 块共同构成同一类型的方法集合，全是「接收外部
//! `Connection`」的无状态方法，故调用方（`crate::db::CategoryRepo::xxx`）与
//! `category_repo_tests.rs` 都无需改动。

use crate::db::models::Category;

const UPSERT_CATEGORY_SQL: &str = "
    INSERT INTO categories (id, name, parent_id, icon, color, sort_order, is_builtin, target_dir)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
    ON CONFLICT(id) DO UPDATE SET
        name = excluded.name,
        parent_id = excluded.parent_id,
        icon = excluded.icon,
        color = excluded.color,
        sort_order = excluded.sort_order,
        is_builtin = excluded.is_builtin,
        target_dir = excluded.target_dir,
        updated_at = datetime('now')
";

const SELECT_ALL_SQL: &str = "
    SELECT id, name, parent_id, icon, color, sort_order, is_builtin, target_dir, created_at, updated_at
    FROM categories
    ORDER BY sort_order ASC, name ASC
";

/// `categories` 表仓库。
pub struct CategoryRepo;

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
        target_dir: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

mod crud;
mod list;
mod seed;

#[cfg(test)]
#[path = "../category_repo_tests.rs"]
mod tests;
