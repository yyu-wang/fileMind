//! 分类写入与删除（原 `category_repo.rs` 拆出）。

use rusqlite::{params, Connection};

use super::{CategoryRepo, UPSERT_CATEGORY_SQL};
use crate::db::models::Category;
use crate::error::{AppError, AppResult};

impl CategoryRepo {
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
                category.target_dir,
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
