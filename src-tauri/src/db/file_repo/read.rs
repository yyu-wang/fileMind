//! 读路径：单条 / 批量 / 条件查询与计数（全部只读，不含任何写入）。

use super::{map_file_record, FileRepo};
use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};

const GET_BY_PATH_SQL: &str = "
    SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
    FROM files WHERE path = ?1 AND is_deleted = 0
";

impl FileRepo {
    /// 按路径查找未删除文件。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误；不存在时返回 `None`。
    pub fn get_by_path(conn: &Connection, path: &str) -> AppResult<Option<FileRecord>> {
        let mut stmt = conn.prepare(GET_BY_PATH_SQL)?;
        let result = stmt.query_row(params![path], map_file_record);

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// 按 ID 查找未删除文件。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误；不存在时返回 `None`。
    pub fn get_by_id(conn: &Connection, id: &str) -> AppResult<Option<FileRecord>> {
        let result = conn.query_row(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE id = ?1 AND is_deleted = 0",
            params![id],
            map_file_record,
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// 批量按 ID 反查文件记录（`preview_operations` 用）。
    ///
    /// 行为：
    ///   - 顺序保证返回顺序与 `ids` 输入顺序一致（`SQL IN (...)` 不保证顺序，
    ///     这里用 `HashMap<id, FileRecord>` 重排）
    ///   - 不存在的 ID 静默跳过（调用方通过 `result.len()` vs `ids.len()` 判断丢失）
    ///   - 已软删除的记录（`is_deleted=1`）也跳过
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn get_by_ids(conn: &Connection, ids: &[String]) -> AppResult<Vec<FileRecord>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE id IN ({placeholders}) AND is_deleted = 0"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), map_file_record)?;

        // SQL IN 不保证顺序，按 ids 顺序重排
        let mut by_id: std::collections::HashMap<String, FileRecord> =
            std::collections::HashMap::new();
        for row in rows {
            let r = row?;
            by_id.insert(r.id.clone(), r);
        }
        let result = ids.iter().filter_map(|id| by_id.remove(id)).collect();
        Ok(result)
    }

    /// 分页列出未删除文件，可按分类过滤，按更新时间倒序。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list(
        conn: &Connection,
        category: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let sql = match category {
            Some(_) => {
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
                 FROM files WHERE is_deleted = 0 AND category = ?1
                 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"
            }
            None => {
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
                 FROM files WHERE is_deleted = 0
                 ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2"
            }
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = match category {
            Some(cat) => stmt.query_map(params![cat, limit, offset], map_file_record)?,
            None => stmt.query_map(params![limit, offset], map_file_record)?,
        };

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    /// 统计未删除文件数量，可按分类过滤。
    ///
    /// # Errors
    ///
    /// 聚合查询失败时返回错误。
    pub fn count(conn: &Connection, category: Option<&str>) -> AppResult<i64> {
        let count: i64 = match category {
            Some(cat) => conn.query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND category = ?1",
                params![cat],
                |row| row.get(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )?,
        };
        Ok(count)
    }

    /// 按内容哈希查找所有未删除文件（用于重复文件分组）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn get_by_hash(conn: &Connection, content_hash: &str) -> AppResult<Vec<FileRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE content_hash = ?1 AND is_deleted = 0"
        )?;

        let rows = stmt.query_map(params![content_hash], map_file_record)?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }
}
