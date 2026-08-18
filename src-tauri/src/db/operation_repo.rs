//! `operations_log` 表数据仓库：批次写入、状态更新与历史聚合。

use rusqlite::{params, Connection};

use crate::db::models::{OperationBatchSummary, OperationLog};
use crate::error::AppResult;

const INSERT_LOG_SQL: &str = "
    INSERT INTO operations_log
        (id, batch_id, operation_type, source_path, target_path, status, prev_hash, current_hash)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
";

/// `operations_log` 表仓库：全部方法接收外部连接，便于事务组合。
pub struct OperationRepo;

impl OperationRepo {
    /// 在单个事务中批量插入操作日志（一个批次的多条行）。
    ///
    /// # Errors
    ///
    /// 事务开启、插入或提交失败时返回错误。
    pub fn insert_batch(conn: &Connection, logs: &[OperationLog]) -> AppResult<usize> {
        let tx = conn.unchecked_transaction()?;
        let mut count = 0;

        for log in logs {
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
                ],
            )?;
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    /// 按批次 ID 查询该批次所有行（撤销时拉取反向操作链用）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list_by_batch(conn: &Connection, batch_id: &str) -> AppResult<Vec<OperationLog>> {
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, operation_type, source_path, target_path,
                    status, prev_hash, current_hash, created_at
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

    /// 按 `batch_id` 聚合历史记录，分页返回批次摘要。
    ///
    /// 实现说明：
    /// - `total_count = COUNT(*)`：批次内日志总条数
    /// - `success_count`/`failed_count`：按 status 分组计数
    /// - `op_type`：取批次内首条行的 `operation_type`（`MIN(created_at)` 对应行）
    /// - `status`：批次状态聚合规则——任一 und 0= undone 即 undone；否则全部 done 即 done；
    ///   任一 failed 即 failed；其余为 pending
    /// - `can_undo`：status='done' 即可撤销（撤销窗口期由调用方再判断）
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
            Ok(OperationBatchSummary {
                batch_id: row.get(0)?,
                op_type: row.get(1)?,
                total_count: row.get(2)?,
                success_count: row.get(3)?,
                failed_count: row.get(4)?,
                can_undo: status == "done",
                status,
                created_at: row.get(6)?,
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
        created_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::database::Database;
    use tempfile::NamedTempFile;

    fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        Ok(Database::open(tmp.path())?)
    }

    fn mk_log(id: &str, batch: &str, op: &str, status: &str) -> OperationLog {
        OperationLog {
            id: id.to_string(),
            batch_id: batch.to_string(),
            operation_type: op.to_string(),
            source_path: "/src/a.txt".to_string(),
            target_path: "/dst/a.txt".to_string(),
            status: status.to_string(),
            prev_hash: "h0".to_string(),
            current_hash: "h1".to_string(),
            created_at: "2026-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn test_insert_and_list_by_batch() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let logs = vec![
            mk_log("l1", "b1", "move", "done"),
            mk_log("l2", "b1", "move", "done"),
            mk_log("l3", "b2", "delete", "done"),
        ];
        let n = OperationRepo::insert_batch(db.conn(), &logs)?;
        assert_eq!(n, 3);

        let b1 = OperationRepo::list_by_batch(db.conn(), "b1")?;
        assert_eq!(b1.len(), 2);
        assert_eq!(b1[0].batch_id, "b1");

        let b2 = OperationRepo::list_by_batch(db.conn(), "b2")?;
        assert_eq!(b2.len(), 1);
        assert_eq!(b2[0].operation_type, "delete");
        Ok(())
    }

    #[test]
    fn test_update_status() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let logs = vec![mk_log("l1", "b1", "move", "pending")];
        OperationRepo::insert_batch(db.conn(), &logs)?;

        OperationRepo::update_status(db.conn(), "l1", "done")?;

        let logs = OperationRepo::list_by_batch(db.conn(), "b1")?;
        assert_eq!(logs[0].status, "done");

        // 不存在的 id → 报错
        let r = OperationRepo::update_status(db.conn(), "nonexistent", "done");
        assert!(r.is_err());
        Ok(())
    }

    #[test]
    fn test_list_history_aggregation() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        // 批次 b1：3 行 done → status='done'，can_undo=true
        let logs = vec![
            mk_log("l1", "b1", "move", "done"),
            mk_log("l2", "b1", "move", "done"),
            mk_log("l3", "b1", "move", "done"),
        ];
        OperationRepo::insert_batch(db.conn(), &logs)?;
        // 批次 b2：1 done + 1 failed → status='failed'
        let logs2 = vec![
            mk_log("l4", "b2", "move", "done"),
            mk_log("l5", "b2", "move", "failed"),
        ];
        OperationRepo::insert_batch(db.conn(), &logs2)?;
        // 批次 b3：2 pending → status='pending'
        let logs3 = vec![
            mk_log("l6", "b3", "delete", "pending"),
            mk_log("l7", "b3", "delete", "pending"),
        ];
        OperationRepo::insert_batch(db.conn(), &logs3)?;

        let summaries = OperationRepo::list_history(db.conn(), 1, 10)?;
        // 按 created_at DESC：3 个批次都返回
        assert_eq!(summaries.len(), 3);

        let by_batch: std::collections::HashMap<String, OperationBatchSummary> = summaries
            .into_iter()
            .map(|s| (s.batch_id.clone(), s))
            .collect();

        let b1 = &by_batch["b1"];
        assert_eq!(b1.total_count, 3);
        assert_eq!(b1.success_count, 3);
        assert_eq!(b1.failed_count, 0);
        assert_eq!(b1.status, "done");
        assert!(b1.can_undo);
        assert_eq!(b1.op_type, "move");

        let b2 = &by_batch["b2"];
        assert_eq!(b2.total_count, 2);
        assert_eq!(b2.success_count, 1);
        assert_eq!(b2.failed_count, 1);
        assert_eq!(b2.status, "failed");
        assert!(!b2.can_undo);

        let b3 = &by_batch["b3"];
        assert_eq!(b3.status, "pending");
        assert!(!b3.can_undo);
        assert_eq!(b3.op_type, "delete");
        Ok(())
    }

    #[test]
    fn test_list_history_pagination() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        // 3 个批次
        for i in 1..=3 {
            let logs = vec![mk_log(&format!("l{i}"), &format!("b{i}"), "move", "done")];
            OperationRepo::insert_batch(db.conn(), &logs)?;
        }

        // page_size=2 → 第一页返回 2 个批次
        let p1 = OperationRepo::list_history(db.conn(), 1, 2)?;
        assert_eq!(p1.len(), 2);
        // 第二页返回剩余 1 个
        let p2 = OperationRepo::list_history(db.conn(), 2, 2)?;
        assert_eq!(p2.len(), 1);
        Ok(())
    }

    #[test]
    fn test_list_history_undone_status() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        // 批次内含 undone → status='undone'
        let logs = vec![
            mk_log("l1", "b1", "move", "done"),
            mk_log("l2", "b1", "move", "undone"),
        ];
        OperationRepo::insert_batch(db.conn(), &logs)?;

        let s = OperationRepo::list_history(db.conn(), 1, 10)?;
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].status, "undone");
        assert!(!s[0].can_undo);
        Ok(())
    }

    #[test]
    fn test_list_by_batch_empty() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let logs = OperationRepo::list_by_batch(db.conn(), "nonexistent")?;
        assert!(logs.is_empty());
        Ok(())
    }

    #[test]
    fn test_list_history_empty() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let summaries = OperationRepo::list_history(db.conn(), 1, 10)?;
        assert!(summaries.is_empty());
        Ok(())
    }
}
