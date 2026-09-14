//! `operation_repo` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests/` 目录）。

#![allow(clippy::unwrap_used, clippy::expect_used)]
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
        chain_hash: String::new(),
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
    // 批次 b1：3 行 done move → status='done', has_delete=false, can_undo=true
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
    // 批次 b3：2 pending delete → status='pending', has_delete=true, can_undo=false
    let logs3 = vec![
        mk_log("l6", "b3", "delete", "pending"),
        mk_log("l7", "b3", "delete", "pending"),
    ];
    OperationRepo::insert_batch(db.conn(), &logs3)?;
    // 批次 b4：2 done delete → status='done' 但含 delete → can_undo=false（DB 层就否决）
    let logs4 = vec![
        mk_log("l8", "b4", "delete", "done"),
        mk_log("l9", "b4", "delete", "done"),
    ];
    OperationRepo::insert_batch(db.conn(), &logs4)?;

    let summaries = OperationRepo::list_history(db.conn(), 1, 10)?;
    // 4 个批次都返回
    assert_eq!(summaries.len(), 4);

    let by_batch: std::collections::HashMap<String, OperationBatchSummary> = summaries
        .into_iter()
        .map(|s| (s.batch_id.clone(), s))
        .collect();

    let b1 = &by_batch["b1"];
    assert_eq!(b1.total_count, 3);
    assert_eq!(b1.success_count, 3);
    assert_eq!(b1.failed_count, 0);
    assert_eq!(b1.status, "done");
    assert!(!b1.has_delete);
    assert!(b1.can_undo, "纯 move done 批次应可撤销");
    assert_eq!(b1.op_type, "move");

    let b2 = &by_batch["b2"];
    assert_eq!(b2.total_count, 2);
    assert_eq!(b2.success_count, 1);
    assert_eq!(b2.failed_count, 1);
    assert_eq!(b2.status, "failed");
    assert!(!b2.has_delete);
    assert!(!b2.can_undo, "failed 批次不可撤销");

    let b3 = &by_batch["b3"];
    assert_eq!(b3.status, "pending");
    assert!(b3.has_delete, "b3 含 delete 操作");
    assert!(!b3.can_undo, "pending 批次不可撤销");
    assert_eq!(b3.op_type, "delete");

    let b4 = &by_batch["b4"];
    assert_eq!(b4.status, "done");
    assert!(b4.has_delete, "b4 含 delete 操作");
    assert!(
        !b4.can_undo,
        "b4 即使 status=done，含 delete 也不可撤销（DB 层否决）"
    );
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

// ------------------------------------------------------------------
// T3.5 链式哈希集成测试
// ------------------------------------------------------------------

#[test]
fn test_insert_batch_computes_chain_hash() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let logs = vec![
        mk_log("l1", "b1", "move", "done"),
        mk_log("l2", "b1", "move", "done"),
    ];
    OperationRepo::insert_batch(db.conn(), &logs)?;

    // 读回：chain_hash 非空且校验通过
    let stored = OperationRepo::list_by_batch(db.conn(), "b1")?;
    assert_eq!(stored.len(), 2);
    assert!(
        stored.iter().all(|l| !l.chain_hash.is_empty()),
        "chain_hash 不应为空"
    );

    // 全表校验通过（None）
    let broken = OperationRepo::verify_chain(db.conn())?;
    assert!(broken.is_none(), "链应完整，实际断裂于 {broken:?}");
    Ok(())
}

#[test]
fn test_verify_chain_detects_tamper() {
    // V007 触发器禁止 UPDATE 不可变字段，这里改用「直接构造被篡改的日志列表」
    // 调 log_chain::verify 验证检测逻辑（DB 层的篡改阻止由 test_trigger_* 覆盖）。
    let logs = vec![
        mk_log("l1", "b1", "move", "done"),
        mk_log("l2", "b1", "move", "done"),
    ];
    // 手工构造合法链
    let c0 = log_chain::hash_record(log_chain::GENESIS, &log_chain::canonicalize(&logs[0]));
    let c1 = log_chain::hash_record(&c0, &log_chain::canonicalize(&logs[1]));
    let mut logs = logs;
    logs[0].chain_hash = c0;
    logs[1].chain_hash = c1;

    // 篡改第二条的 source_path，但保留原 chain_hash
    logs[1].source_path = "/tampered.txt".to_string();

    assert_eq!(log_chain::verify(&logs), Some(1), "应检测到 l2 处断裂");
}

#[test]
fn test_verify_chain_empty_table_ok() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_db()?;
    let broken = OperationRepo::verify_chain(db.conn())?;
    assert!(broken.is_none());
    Ok(())
}

// ------------------------------------------------------------------
// T3.5 V007 触发器行为测试（INSERT-only 强制）
// ------------------------------------------------------------------

#[test]
fn test_trigger_blocks_update_non_status() -> Result<(), Box<dyn std::error::Error>> {
    // 模拟攻击者绕过应用层，直接执行 UPDATE source_path → 应被触发器拒绝
    let db = setup_db()?;
    let logs = vec![mk_log("l1", "b1", "move", "done")];
    OperationRepo::insert_batch(db.conn(), &logs)?;

    let result = db.conn().execute(
        "UPDATE operations_log SET source_path = ?1 WHERE id = ?2",
        params!["/tampered.txt", "l1"],
    );
    assert!(result.is_err(), "UPDATE source_path 应被触发器拒绝");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("仅允许更新 status"),
        "错误信息应说明原因: {err_msg}"
    );

    // 校验记录未被修改
    let stored = OperationRepo::list_by_batch(db.conn(), "b1")?;
    assert_eq!(stored[0].source_path, "/src/a.txt");
    Ok(())
}

#[test]
fn test_trigger_allows_update_status() -> Result<(), Box<dyn std::error::Error>> {
    // undo_batch 的 done→undone 状态变更应被允许（status 不在触发器保护列表）
    let db = setup_db()?;
    let logs = vec![mk_log("l1", "b1", "move", "done")];
    OperationRepo::insert_batch(db.conn(), &logs)?;

    OperationRepo::update_status(db.conn(), "l1", "undone")?;

    // 校验状态已变 + 链仍完整（status 不参与 canonicalize）
    let stored = OperationRepo::list_by_batch(db.conn(), "b1")?;
    assert_eq!(stored[0].status, "undone");
    let broken = OperationRepo::verify_chain(db.conn())?;
    assert!(broken.is_none(), "状态变更不应破坏链");
    Ok(())
}

#[test]
fn test_trigger_blocks_delete() -> Result<(), Box<dyn std::error::Error>> {
    // 模拟攻击者绕过应用层，直接 DELETE → 应被触发器拒绝
    let db = setup_db()?;
    let logs = vec![mk_log("l1", "b1", "move", "done")];
    OperationRepo::insert_batch(db.conn(), &logs)?;

    let result = db
        .conn()
        .execute("DELETE FROM operations_log WHERE id = ?1", params!["l1"]);
    assert!(result.is_err(), "DELETE 应被触发器拒绝");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("禁止删除"),
        "错误信息应说明原因: {err_msg}"
    );

    // 校验记录仍存在
    let stored = OperationRepo::list_by_batch(db.conn(), "b1")?;
    assert_eq!(stored.len(), 1, "记录不应被删除");
    Ok(())
}
