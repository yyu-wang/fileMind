use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::models::FileRecord;
use crate::error::AppResult;

pub struct FileSearch;

impl FileSearch {
    pub fn search(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<SearchResult>> {
        let sanitized = sanitize_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let fts_query = format!("\"{}\"*", sanitized);

        let mut stmt = conn.prepare(
            "SELECT f.id, f.path, f.file_name, f.file_size, f.content_hash,
                    f.category, f.is_deleted, f.created_at, f.updated_at,
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
                },
                score: row.get(9)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn search_by_filename(
        conn: &Connection,
        pattern: &str,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let like_pattern = format!("%{pattern}%");

        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash,
                    category, is_deleted, created_at, updated_at
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
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SearchResult {
    pub file: FileRecord,
    pub score: f64,
}

fn sanitize_query(query: &str) -> String {
    query
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '_')
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
        assert_eq!(sanitize_query("hello world"), "hello world");
        assert_eq!(sanitize_query("hello; DROP TABLE"), "hello DROP TABLE");
        assert_eq!(sanitize_query(""), "");
    }
}
