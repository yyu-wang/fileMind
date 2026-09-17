//! FTS5 全文搜索与文件名 LIKE 搜索（原 `file_search.rs` 拆出）。

use rusqlite::{params, Connection};

use super::query::{build_fts_query, sanitize_query};
use super::{FileSearch, SearchResult};
use crate::db::models::FileRecord;
use crate::error::AppResult;

impl FileSearch {
    /// FTS5 全文搜索，按 bm25 相关度升序返回。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn search(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<SearchResult>> {
        let sanitized = sanitize_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        // 构造 FTS5 查询：对长查询截断到前 2~3 字做前缀匹配，
        // 避免整句作为单 token 导致无匹配（unicode61 把连续 CJK 合并为一个 token）。
        let fts_query = build_fts_query(&sanitized);

        let mut stmt = conn.prepare(
            "SELECT f.id, f.path, f.file_name, f.file_size, f.content_hash,
                    f.category, f.is_deleted, f.created_at, f.updated_at, f.mtime,
                    bm25(file_fts) as score
             FROM file_fts
             JOIN files f ON f.id = file_fts.file_id
             WHERE file_fts MATCH ?1 AND f.is_deleted = 0
             ORDER BY score
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![fts_query, limit], |row| {
            Ok(SearchResult {
                file: FileRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    file_name: row.get(2)?,
                    file_size: row.get(3)?,
                    content_hash: row.get(4)?,
                    category: row.get(5)?,
                    is_deleted: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    mtime: row.get(9)?,
                },
                score: row.get(10)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// 按文件名子串模糊搜索，按更新时间倒序返回。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn search_by_filename(
        conn: &Connection,
        pattern: &str,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let like_pattern = format!("%{pattern}%");

        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash,
                    category, is_deleted, created_at, updated_at, mtime
             FROM files
             WHERE is_deleted = 0 AND file_name LIKE ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![like_pattern, limit], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                file_name: row.get(2)?,
                file_size: row.get(3)?,
                content_hash: row.get(4)?,
                category: row.get(5)?,
                is_deleted: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                mtime: row.get(9)?,
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }
}
