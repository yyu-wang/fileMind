//! 向量化标记：索引增量的「待建 / 已建」查询与回写。

use super::{map_file_record, FileRepo};
use crate::db::models::FileRecord;
use crate::error::AppResult;
use rusqlite::{params, Connection};

impl FileRepo {
    /// 列出待向量化文件（增量索引的候选集）。
    ///
    /// 未删除且满足任一条件即入选：
    ///   - `embedding_model IS NULL`（从未建过索引）
    ///   - `embedding_model != 当前模型`（Embedding 模型已切换）
    ///   - `embedding_hash IS NULL`（建索引时未记录内容哈希）
    ///   - `embedding_hash != content_hash`（内容在索引后发生过变更）
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn list_pending_embedding(
        conn: &Connection,
        embedding_model: &str,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files
             WHERE is_deleted = 0
               AND (embedding_model IS NULL
                    OR embedding_model <> ?1
                    OR embedding_hash IS NULL
                    OR embedding_hash <> content_hash)
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![embedding_model, limit], map_file_record)?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    /// 向量化成功后回写状态标记（`embedding_model` + `embedding_hash`）。
    ///
    /// `entries` 为 `(file_id, 建索引时刻的 content_hash)` 列表；全部 UPDATE 包在
    /// 单事务中执行。调用方只应传入「实际写入向量的文件」，读取失败 / 非文本
    /// 文件保持未标记，下次索引自动重试。
    ///
    /// # Errors
    ///
    /// 事务开启或任一更新失败时返回数据库错误。
    pub fn mark_embedded(
        conn: &Connection,
        embedding_model: &str,
        entries: &[(String, String)],
    ) -> AppResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let tx = conn.unchecked_transaction()?;
        let mut count = 0;
        for (file_id, content_hash) in entries {
            let affected = tx.execute(
                "UPDATE files
                 SET embedding_model = ?1,
                     embedding_hash = ?2,
                     updated_at = datetime('now')
                 WHERE id = ?3 AND is_deleted = 0",
                params![embedding_model, content_hash, file_id],
            )?;
            count += affected;
        }
        tx.commit()?;
        Ok(count)
    }
}
