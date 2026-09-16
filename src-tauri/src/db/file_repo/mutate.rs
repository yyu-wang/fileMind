//! 写路径（字段级）：分类与路径更新、软删除（单条 / 按路径前缀）、向量化标记清理。

use super::{FileRepo, CHUNK_IN_PATHS};
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use std::path::Path;

impl FileRepo {
    /// 更新指定文件的分类。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn update_category(conn: &Connection, id: &str, category: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET category = ?1, updated_at = datetime('now') WHERE id = ?2 AND is_deleted = 0",
            params![category, id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 移动/重命名后更新文件路径 + `updated_at`（`execute_operations` 用）。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn update_path(conn: &Connection, id: &str, new_path: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET path = ?1, file_name = ?2, updated_at = datetime('now')
             WHERE id = ?3 AND is_deleted = 0",
            params![
                new_path,
                Path::new(new_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                id
            ],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 软删除指定文件（置 `is_deleted` 标记）。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn soft_delete(conn: &Connection, id: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET is_deleted = 1, category = NULL, updated_at = datetime('now')
             WHERE id = ?1",
            params![id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 按路径前缀批量软删除（目录级移除用）。
    ///
    /// 匹配规则 `path LIKE prefix || '/%'`，避免 `/a/b` 误匹配 `/a/bc`。
    /// 返回受影响的行数；无匹配时返回 0（不报错，幂等）。
    ///
    /// 语义：目录级移除 = 放弃 `FileMind` 对该目录的全部派生状态。因此连同
    /// `category` 分类标签一起清空——否则重新扫描该目录时 `ON CONFLICT(path)`
    /// 复活记录会保留旧标签，文件明明还在原地却显示「已分类」，永远不再被
    /// 「未整理」类整理入口处理。`content_hash` 保留（增量扫描可复用，无副作用）。
    ///
    /// # Errors
    ///
    /// 更新失败时返回数据库错误。
    pub fn soft_delete_by_path_prefix(conn: &Connection, path_prefix: &str) -> AppResult<i64> {
        let affected = conn.execute(
            "UPDATE files SET is_deleted = 1, category = NULL, updated_at = datetime('now')
             WHERE is_deleted = 0 AND path LIKE ?1 || '/%'",
            params![path_prefix],
        )?;
        i64::try_from(affected).map_err(|e| AppError::Internal(format!("行数转换失败: {e}")))
    }

    /// 清除指定文件的索引状态标记（向量已从 `LanceDB` 删除时调用）。
    ///
    /// 把 `embedding_model` / `embedding_hash` 置回 NULL，避免「向量已删但标记仍在」
    /// 导致增量索引把该文件误判为已建而跳过。ID 列表按 [`CHUNK_IN_PATHS`] 分块，
    /// 用单条 `UPDATE ... IN (...)` 批量执行。
    ///
    /// # Errors
    ///
    /// 任一 UPDATE 失败时返回数据库错误。
    pub fn clear_embedding_marker(conn: &Connection, ids: &[String]) -> AppResult<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut cleared = 0;
        for chunk in ids.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "UPDATE files
                 SET embedding_model = NULL,
                     embedding_hash = NULL,
                     updated_at = datetime('now')
                 WHERE id IN ({placeholders})"
            );
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            cleared += conn.execute(&sql, params.as_slice())?;
        }
        Ok(cleared)
    }
}
