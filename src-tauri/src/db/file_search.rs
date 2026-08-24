//! 文件搜索：FTS5 全文搜索与文件名模糊搜索。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::models::FileRecord;
use crate::error::AppResult;

/// 搜索入口：全文搜索走 FTS5，文件名搜索走 LIKE。
pub struct FileSearch;

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

        let fts_query = format!("\"{sanitized}\"*");

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

/// 全文搜索命中结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SearchResult {
    /// 命中的文件记录。
    pub file: FileRecord,
    /// bm25 相关度得分（越小越相关）。
    pub score: f64,
}

/// 清洗用户查询：保留 Unicode 字母数字（含中文/日文/韩文）、空白与下划线。
///
/// 安全说明：
/// - 移除 FTS5 特殊字符（``*`` ``"`` ``(`` ``)`` ``OR`` 等），防止语法注入
/// - ``is_alphanumeric`` 走 Unicode ``Alphabetic`` / ``Numeric`` 属性，CJK 字符
///   在该属性中为 true，因此中文查询能透传到 FTS5 MATCH
/// - ``is_whitespace`` 允许任意 Unicode 空白（含全角空格、制表符），便于多 token 查询
/// - 保留下划线 ``_``：常见于文件名（如 ``my_report.pdf``），FTS5 视为普通字符
fn sanitize_query(query: &str) -> String {
    query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::database::Database;
    use crate::db::file_repo::FileRepo;
    use tempfile::NamedTempFile;

    fn setup_test_db_with_data() -> Result<Database, Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        let db = Database::open(tmp.path())?;

        let files = vec![
            FileRecord {
                id: "f001".to_string(),
                path: "/docs/report.pdf".to_string(),
                file_name: "report.pdf".to_string(),
                file_size: 1024,
                content_hash: Some("hash1".to_string()),
                category: Some("文档".to_string()),
                is_deleted: false,
                created_at: "2026-01-01".to_string(),
                updated_at: "2026-01-01".to_string(),
                mtime: None,
            },
            FileRecord {
                id: "f002".to_string(),
                path: "/docs/notes.md".to_string(),
                file_name: "notes.md".to_string(),
                file_size: 512,
                content_hash: Some("hash2".to_string()),
                category: Some("文档".to_string()),
                is_deleted: false,
                created_at: "2026-01-02".to_string(),
                updated_at: "2026-01-02".to_string(),
                mtime: None,
            },
        ];

        FileRepo::insert_batch(db.conn(), &files)?;
        Ok(db)
    }

    #[test]
    fn test_search_by_filename() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_test_db_with_data()?;
        let results = FileSearch::search_by_filename(db.conn(), "report", 10)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "report.pdf");
        Ok(())
    }

    #[test]
    fn test_search_by_filename_no_match() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_test_db_with_data()?;
        let results = FileSearch::search_by_filename(db.conn(), "nonexistent", 10)?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn test_search_empty_query() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_test_db_with_data()?;
        let results = FileSearch::search(db.conn(), "", 10)?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn test_sanitize_query() {
        // 英文：保留字母数字与空格
        assert_eq!(sanitize_query("hello world"), "hello world");
        // 注入防御：FTS5 特殊字符 ``;`` 被过滤
        assert_eq!(sanitize_query("hello; DROP TABLE"), "hello DROP TABLE");
        assert_eq!(sanitize_query(""), "");
        // 中文：is_alphanumeric 对 CJK 返回 true，应原样保留
        assert_eq!(sanitize_query("文件管理"), "文件管理");
        assert_eq!(sanitize_query("FileMind 文件管理"), "FileMind 文件管理");
        // 标点过滤：`,` `!` 等被移除，空格保留
        assert_eq!(sanitize_query("hello, world!"), "hello world");
        // 下划线保留：常见于文件名
        assert_eq!(sanitize_query("my_report"), "my_report");
        // FTS5 通配符与引号被过滤，防止语法注入
        assert_eq!(sanitize_query("hello*\" OR 1=1"), "hello OR 11");
    }
}
