//! 按路径前缀批量查询：目录管理（移除目录）与批量执行（分类回填、快照加载）用。

use super::{FileRepo, FileSnapshot, CHUNK_IN_PATHS};
use crate::error::AppResult;
use rusqlite::{params, Connection};
use std::collections::HashMap;

impl FileRepo {
    /// 按路径前缀获取所有未软删除文件的 ID（清理向量索引用）。
    ///
    /// 匹配规则同 [`Self::soft_delete_by_path_prefix`]。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn get_ids_by_path_prefix(conn: &Connection, path_prefix: &str) -> AppResult<Vec<String>> {
        let mut stmt =
            conn.prepare("SELECT id FROM files WHERE is_deleted = 0 AND path LIKE ?1 || '/%'")?;
        let ids = stmt
            .query_map(params![path_prefix], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// 按路径批量回查既存分类（扫描后回显「已整理」状态用）。
    ///
    /// 返回 `path → category`，仅包含库中存在且未软删除、且已有分类的路径。
    /// `scan_directory` 扫描到的文件 category 恒为 `None`，但同一路径此前若被整理过，
    /// DB 里已存有分类；这里按 path 回查后覆盖回返回结果，前端才能正确标记。
    ///
    /// 路径数超过单条 SQL 变量上限时按 [`CHUNK_IN_PATHS`] 分块（T10.1）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn get_categories_by_paths(
        conn: &Connection,
        paths: &[String],
    ) -> AppResult<HashMap<String, String>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let mut categories = HashMap::new();
        for chunk in paths.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT path, category FROM files
                 WHERE path IN ({placeholders}) AND is_deleted = 0 AND category IS NOT NULL"
            );

            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;

            for row in rows {
                let (path, category) = row?;
                categories.insert(path, category);
            }
        }
        Ok(categories)
    }

    /// 批量加载增量扫描快照（T10.1）。
    ///
    /// 返回 `path → FileSnapshot`，**包含软删除行**（与 `get_categories_by_paths` 不同，
    /// 软删记录需复活，不能过滤）。路径数超过单条 SQL 变量上限时按 [`CHUNK_IN_PATHS`]
    /// 分块，替代旧的逐条 `SELECT ... WHERE path=?1`（`upsert_batch` 曾每文件一条查询）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn load_snapshots_by_paths(
        conn: &Connection,
        paths: &[String],
    ) -> AppResult<HashMap<String, FileSnapshot>> {
        let mut snapshots = HashMap::with_capacity(paths.len());
        for chunk in paths.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT path, id, is_deleted, content_hash, file_size, mtime
                 FROM files WHERE path IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileSnapshot {
                        id: row.get(1)?,
                        is_deleted: row.get::<_, i64>(2)? != 0,
                        content_hash: row.get(3)?,
                        file_size: row.get(4)?,
                        mtime: row.get(5)?,
                    },
                ))
            })?;
            for row in rows {
                let (path, snapshot) = row?;
                snapshots.insert(path, snapshot);
            }
        }
        Ok(snapshots)
    }
}
