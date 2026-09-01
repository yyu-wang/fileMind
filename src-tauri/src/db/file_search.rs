//! 文件搜索：FTS5 全文搜索与文件名模糊搜索。
//! 以及 FTS5 内容填充（文件正文入索引，支撑关键词检索）。

use std::fs;
use std::io::Read;
use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};

/// 单文件读取上限（2MB，对齐 Python sidecar `MAX_FILE_BYTES`）。
const MAX_FTS_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 文本文件扩展名白名单（与 Python sidecar 保持一致）。
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "log", "json", "yaml", "yml", "csv", "xml", "html", "htm", "css",
    "js", "ts", "tsx", "py", "rs", "go", "java", "c", "h", "cpp", "hpp", "sql", "ini", "cfg",
    "toml", "sh", "bash", "zsh", "conf", "jsx", "bat",
];

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

    /// 填充 FTS5 content 列：读取文件正文并写入索引，支撑关键词检索。
    ///
    /// 逐个读取文件内容，对 FTS5 表执行「删旧→插新」保证幂等。
    /// 非文本文件 / 读取失败文件跳过，不中断整体流程。
    ///
    /// # Arguments
    ///
    /// * `conn` - `SQLite` 连接
    /// * `files` - 需要填充的文件记录列表
    ///
    /// # Returns
    ///
    /// `(indexed_count, skipped_count)` — 成功读取的文件数 / 跳过的文件数
    ///
    /// # Errors
    ///
    /// FTS5 删旧 / 插新 SQL 执行失败时返回 `Database` 错误。
    pub fn populate_fts_content(conn: &Connection, files: &[FileRecord]) -> AppResult<(i64, i64)> {
        let mut indexed: i64 = 0;
        let mut skipped: i64 = 0;

        for file in files {
            let path = Path::new(&file.path);
            if !path.is_file() {
                skipped += 1;
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase)
                .unwrap_or_default();

            if !TEXT_EXTENSIONS.contains(&ext.as_str()) {
                skipped += 1;
                continue;
            }

            let Ok(metadata) = fs::metadata(path) else {
                skipped += 1;
                continue;
            };

            if metadata.len() > MAX_FTS_FILE_BYTES {
                skipped += 1;
                continue;
            }

            let Ok(mut file_handle) = fs::File::open(path) else {
                skipped += 1;
                continue;
            };

            let mut contents = String::new();
            if file_handle.read_to_string(&mut contents).is_err() {
                skipped += 1;
                continue;
            }

            if contents.trim().is_empty() {
                skipped += 1;
                continue;
            }

            // FTS5 contentless 表：先删后插（幂等）
            conn.execute("DELETE FROM file_fts WHERE file_id = ?1", params![file.id])
                .map_err(AppError::Database)?;

            conn.execute(
                "INSERT INTO file_fts(file_id, file_name, content, path) VALUES (?1, ?2, ?3, ?4)",
                params![file.id, file.file_name, contents, file.path],
            )
            .map_err(AppError::Database)?;

            indexed += 1;
        }

        Ok((indexed, skipped))
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

/// 构造 FTS5 查询字符串：对长 CJK 查询截断到前 2 字做前缀匹配。
///
/// 背景：sqlite FTS5 的 ``unicode61`` 分词器把连续 CJK 字符合并为一个 token。
/// 若整句作为前缀搜索（如 ``"分类整理的流程是什么"*``），需要文档里存在
/// 完整的长 token 才能命中——实际文档极少出现整句，导致检索为零。
/// 截断到前 2 字后（``"分类"*``），以词缀匹配即可找到包含该前缀的所有 token。
///
/// 策略：
/// - 包含 CJK 字符的查询（codepoint > 127）：取前 2 字做前缀
/// - 纯 ASCII 查询：保持整句前缀匹配（英文空格天然分词，整句效果好）
fn build_fts_query(sanitized: &str) -> String {
    if sanitized.is_empty() {
        return String::new();
    }
    let has_cjk = sanitized.chars().any(|c| c as u32 > 127);
    if has_cjk {
        let prefix: String = sanitized.chars().take(2).collect();
        format!("\"{prefix}\"*")
    } else {
        format!("\"{sanitized}\"*")
    }
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

    #[test]
    fn test_build_fts_query() {
        // 空字符串
        assert_eq!(build_fts_query(""), "");
        // 纯 ASCII 查询：保持整句前缀匹配
        assert_eq!(build_fts_query("hello"), "\"hello\"*");
        assert_eq!(build_fts_query("hello world"), "\"hello world\"*");
        // 含 CJK 字符：取前 2 字作前缀
        assert_eq!(build_fts_query("分类"), "\"分类\"*");
        assert_eq!(build_fts_query("文件管理"), "\"文件\"*");
        assert_eq!(build_fts_query("分类整理的流程是什么"), "\"分类\"*");
    }

    /// 构造指向真实临时文件的 `FileRecord`。
    fn record_for(id: &str, path: &Path, file_name: &str, is_deleted: bool) -> FileRecord {
        FileRecord {
            id: id.to_string(),
            path: path.to_string_lossy().to_string(),
            file_name: file_name.to_string(),
            file_size: 100,
            content_hash: None,
            category: None,
            is_deleted,
            created_at: "2026-01-01".to_string(),
            updated_at: "2026-01-01".to_string(),
            mtime: None,
        }
    }

    #[test]
    fn test_populate_fts_content_index_and_skip_branches() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp = NamedTempFile::new()?;
        let db = Database::open(tmp.path())?;
        let dir = tempfile::tempdir()?;

        // 1) 正常文本文件 → indexed
        let md_path = dir.path().join("a.md");
        std::fs::write(&md_path, "FileMind 文件管理 整理分类流程")?;
        // 2) 非文本扩展名 → skipped
        let bin_path = dir.path().join("b.exe");
        std::fs::write(&bin_path, "binary")?;
        // 3) 路径不存在（is_file=false）→ skipped
        let ghost_path = dir.path().join("ghost.md");
        // 4) 空白内容 → skipped
        let empty_path = dir.path().join("empty.txt");
        std::fs::write(&empty_path, "   ")?;
        // 5) 超过 2MB 上限 → skipped
        let big_path = dir.path().join("big.txt");
        let big_len = usize::try_from(MAX_FTS_FILE_BYTES + 1)?;
        std::fs::write(&big_path, vec![b'x'; big_len])?;

        let files = vec![
            record_for("f100", &md_path, "a.md", false),
            record_for("f101", &bin_path, "b.exe", false),
            record_for("f102", &ghost_path, "ghost.md", false),
            record_for("f103", &empty_path, "empty.txt", false),
            record_for("f104", &big_path, "big.txt", false),
        ];
        FileRepo::insert_batch(db.conn(), &files)?;

        let (indexed, skipped) = FileSearch::populate_fts_content(db.conn(), &files)?;
        assert_eq!(indexed, 1);
        assert_eq!(skipped, 4);

        // FTS 全文搜索命中已入库正文（CJK 前 2 字前缀匹配）
        let results = FileSearch::search(db.conn(), "整理", 10)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file.file_name, "a.md");
        assert_eq!(results[0].file.id, "f100");
        Ok(())
    }

    #[test]
    fn test_populate_fts_content_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        let db = Database::open(tmp.path())?;
        let dir = tempfile::tempdir()?;

        let md_path = dir.path().join("a.md");
        std::fs::write(&md_path, "知识库检索测试")?;
        let file = record_for("f200", &md_path, "a.md", false);
        FileRepo::insert_batch(db.conn(), std::slice::from_ref(&file))?;

        // 同一文件重复填充：删旧插新，不产生重复行
        FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;
        let (indexed_again, _) =
            FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;

        assert_eq!(indexed_again, 1);
        let results = FileSearch::search(db.conn(), "知识", 10)?;
        assert_eq!(results.len(), 1);
        Ok(())
    }

    #[test]
    fn test_search_excludes_deleted_files() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        let db = Database::open(tmp.path())?;
        let dir = tempfile::tempdir()?;

        let md_path = dir.path().join("deleted.md");
        std::fs::write(&md_path, "已删除文件内容")?;
        let file = record_for("f300", &md_path, "deleted.md", true);
        FileRepo::insert_batch(db.conn(), std::slice::from_ref(&file))?;

        let (indexed, _) =
            FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;
        assert_eq!(indexed, 1); // FTS 填充不看软删除标记

        // 全文搜索过滤 is_deleted=1
        let results = FileSearch::search(db.conn(), "删除", 10)?;
        assert!(results.is_empty());
        Ok(())
    }
}
