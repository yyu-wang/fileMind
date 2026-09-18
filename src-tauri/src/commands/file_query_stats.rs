//! 文件库统计的 SQL 聚合（原 `file_query.rs` 拆出）。
//!
//! 四条 COUNT / SUM 查询共用同一连接的快照语义（同一次命令调用内计数一致），
//! 独立成模块便于按查询单独演进（加索引、或合并成单条 SQL 聚合）。

use crate::commands::file_query_types::FileStats;
use crate::error::AppResult;

/// 查询文件库统计：总数 / 已分类 / 疑似重复组 / 总大小。
///
/// # Errors
///
/// 任一聚合查询失败时返回数据库错误（命令层转 `DB-U-001`）。
pub(super) fn query_stats(conn: &rusqlite::Connection) -> AppResult<FileStats> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )?;

    let categorized: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND category IS NOT NULL",
        [],
        |row| row.get(0),
    )?;

    let duplicates: i64 = conn.query_row(
        "SELECT COUNT(*) - COUNT(DISTINCT content_hash) FROM files
             WHERE is_deleted = 0 AND content_hash IS NOT NULL",
        [],
        |row| row.get(0),
    )?;

    let total_size: i64 = conn.query_row(
        "SELECT COALESCE(SUM(file_size), 0) FROM files WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )?;

    Ok(FileStats {
        total_files: total,
        categorized_files: categorized,
        uncategorized_files: total - categorized,
        duplicate_groups: duplicates,
        total_size_bytes: total_size,
    })
}
