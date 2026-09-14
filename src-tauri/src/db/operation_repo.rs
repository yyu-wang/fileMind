//! `operations_log` 表数据仓库：批次写入、状态更新与历史聚合。

use rusqlite::{params, Connection};

use crate::db::models::{OperationBatchSummary, OperationLog};
use crate::error::AppResult;
use crate::services::log_chain;

const INSERT_LOG_SQL: &str = "
    INSERT INTO operations_log
        (id, batch_id, operation_type, source_path, target_path, status,
         prev_hash, current_hash, chain_hash, created_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
";

/// `operations_log` 表仓库：全部方法接收外部连接，便于事务组合。
pub struct OperationRepo;

impl OperationRepo {
    /// 在单个事务中批量插入操作日志，并逐条计算链式哈希。
    ///
    /// 链式哈希起点为本批之前最后一条记录的 `chain_hash`（空表时为创世值），
    /// 事务内顺序计算，保证与 `verify_chain` 的 `rowid` 顺序一致。
    ///
    /// # Errors
    ///
    /// 事务开启、插入或提交失败时返回错误。
    pub fn insert_batch(conn: &Connection, logs: &[OperationLog]) -> AppResult<usize> {
        let tx = conn.unchecked_transaction()?;
        let mut prev_chain = Self::last_chain_hash(&tx)?;
        let mut count = 0;

        for log in logs {
            let chain_hash = log_chain::hash_record(&prev_chain, &log_chain::canonicalize(log));
            tx.execute(
                INSERT_LOG_SQL,
                params![
                    log.id,
                    log.batch_id,
                    log.operation_type,
                    log.source_path,
                    log.target_path,
                    log.status,
                    log.prev_hash,
                    log.current_hash,
                    chain_hash,
                    log.created_at,
                ],
            )?;
            prev_chain = chain_hash;
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    /// 读取当前最后一条记录的链式哈希作为下一批插入的起点。
    ///
    /// # Errors
    ///
    /// 查询失败时返回数据库错误；空表返回创世值。
    fn last_chain_hash(conn: &Connection) -> AppResult<String> {
        let result = conn.query_row(
            "SELECT chain_hash FROM operations_log ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(hash) => Ok(hash),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(log_chain::GENESIS.to_string()),
            Err(e) => Err(crate::error::AppError::Database(e)),
        }
    }

    /// 按批次 ID 查询该批次所有行（撤销时拉取反向操作链用）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_by_batch(conn: &Connection, batch_id: &str) -> AppResult<Vec<OperationLog>> {
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, operation_type, source_path, target_path,
                    status, prev_hash, current_hash, chain_hash, created_at
             FROM operations_log
             WHERE batch_id = ?1
             ORDER BY created_at ASC",
        )?;

        let rows = stmt.query_map(params![batch_id], map_operation_log)?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    /// 更新单条日志状态（pending/done/failed/undone）。
    ///
    /// # Errors
    ///
    /// 日志不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn update_status(conn: &Connection, id: &str, status: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE operations_log SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;

        if affected == 0 {
            return Err(crate::error::AppError::Database(
                rusqlite::Error::QueryReturnedNoRows,
            ));
        }
        Ok(())
    }

    /// 按插入顺序（`rowid`）读取全部操作日志，供链式哈希校验。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_all_ordered(conn: &Connection) -> AppResult<Vec<OperationLog>> {
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, operation_type, source_path, target_path,
                    status, prev_hash, current_hash, chain_hash, created_at
             FROM operations_log
             ORDER BY rowid ASC",
        )?;

        let rows = stmt.query_map([], map_operation_log)?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    /// 启动时校验操作日志链完整性。
    ///
    /// 返回第一条断裂记录的 `id`（`None` 表示链完整）。空表视为完整。
    ///
    /// # Errors
    ///
    /// 读取日志失败时返回错误。
    pub fn verify_chain(conn: &Connection) -> AppResult<Option<String>> {
        let logs = Self::list_all_ordered(conn)?;
        Ok(log_chain::verify(&logs).map(|index| logs[index].id.clone()))
    }

    /// 按 `batch_id` 聚合历史记录，分页返回批次摘要。
    ///
    /// 实现说明：
    /// - `total_count = COUNT(*)`：批次内日志总条数
    /// - `success_count`/`failed_count`：按 status 分组计数
    /// - `op_type`：取批次内首条行的 `operation_type`（`MIN(created_at)` 对应行）
    /// - `status`：批次状态聚合规则——任一 undone → undone；否则全部 done → done；
    ///   任一 failed → failed；其余 pending
    /// - `has_delete`：批次内是否含 `delete` 操作（删除已移入系统回收站，
    ///   应用内无回收站还原路径，含则不可撤销，可从系统回收站手动恢复）
    /// - `can_undo`（DB 层）：`status='done' && !has_delete`；撤销窗口期由 IPC 命令层再注入
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_history(
        conn: &Connection,
        page: i64,
        page_size: i64,
    ) -> AppResult<Vec<OperationBatchSummary>> {
        // 聚合 SQL：按 batch_id 分组，计算各类计数
        // 批次状态：用 CASE 表达式实现"任一 undone → undone；任一 failed → failed；
        //           全部 done → done；否则 pending"
        // has_delete：批次内是否含 delete 操作
        let sql = "
            SELECT
                batch_id,
                (SELECT operation_type FROM operations_log
                 WHERE batch_id = o.batch_id
                 ORDER BY created_at ASC LIMIT 1) AS op_type,
                COUNT(*) AS total_count,
                SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_count,
                CASE
                    WHEN SUM(CASE WHEN status = 'undone' THEN 1 ELSE 0 END) > 0 THEN 'undone'
                    WHEN SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) > 0 THEN 'failed'
                    WHEN SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) > 0 THEN 'pending'
                    ELSE 'done'
                END AS batch_status,
                SUM(CASE WHEN operation_type = 'delete' THEN 1 ELSE 0 END) AS delete_count,
                MIN(created_at) AS created_at
            FROM operations_log o
            GROUP BY batch_id
            ORDER BY created_at DESC
            LIMIT ?1 OFFSET ?2
        ";

        let mut stmt = conn.prepare(sql)?;
        let offset = (page - 1).max(0) * page_size;
        let rows = stmt.query_map(params![page_size, offset], |row| {
            let status: String = row.get(5)?;
            let delete_count: i64 = row.get(6)?;
            let has_delete = delete_count > 0;
            Ok(OperationBatchSummary {
                batch_id: row.get(0)?,
                op_type: row.get(1)?,
                total_count: row.get(2)?,
                success_count: row.get(3)?,
                failed_count: row.get(4)?,
                can_undo: status == "done" && !has_delete,
                status,
                created_at: row.get(7)?,
                has_delete,
            })
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(row?);
        }
        Ok(summaries)
    }
}

/// 将查询行映射为 [`OperationLog`]。
fn map_operation_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationLog> {
    Ok(OperationLog {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        operation_type: row.get(2)?,
        source_path: row.get(3)?,
        target_path: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        status: row.get(5)?,
        prev_hash: row.get(6)?,
        current_hash: row.get(7)?,
        chain_hash: row.get(8)?,
        created_at: row.get(9)?,
    })
}

#[cfg(test)]
#[path = "operation_repo_tests.rs"]
mod tests;
