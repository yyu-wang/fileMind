//! FTS5 内容填充（原 `file_search.rs` 拆出）。

use std::fs;
use std::io::Read;
use std::path::Path;

use rusqlite::{params, Connection};

use super::FileSearch;
use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};

/// 单文件读取上限（50MB，对齐 Python sidecar `MAX_FILE_BYTES`）。
pub(super) const MAX_FTS_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// 文本文件扩展名白名单（与 Python sidecar 保持一致）。
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "log", "json", "yaml", "yml", "csv", "xml", "html", "htm", "css",
    "js", "ts", "tsx", "py", "rs", "go", "java", "c", "h", "cpp", "hpp", "sql", "ini", "cfg",
    "toml", "sh", "bash", "zsh", "conf", "jsx", "bat",
];

impl FileSearch {
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
