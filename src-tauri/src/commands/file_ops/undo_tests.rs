//! 撤销与删除测试：`undo_batch` 的移动回滚/复制清理/拒绝规则，以及 `delete_files` 回收站语义。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::test_support::{make_test_app_state, preview_then_execute, seed_files_for_preview};
use super::*;

// 拆分后按需显式引入：内部实现来自子模块，外部名字原先靠 `use super::*` 取到
use super::delete::delete_files_inner;
use super::undo::undo_batch_inner;
use crate::db::{FileRepo, OperationRepo};
use crate::error::AppError;

#[test]
fn test_undo_batch_move_full_flow() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (_, exec) = preview_then_execute(
        &state,
        scan_root.path(),
        &["a.txt", "b.txt"],
        OperationType::Move,
        Some(target_dir.path().to_string_lossy().to_string()),
        vec![],
    )?;
    assert_eq!(exec.summary.success, 2);

    // 执行后：源目录空，目标目录有文件
    assert!(!scan_root.path().join("a.txt").exists());
    assert!(target_dir.path().join("a.txt").exists());

    let undo = undo_batch_inner(&exec.batch_id, &state)?;
    assert!(undo.success);
    assert_eq!(undo.undone_count, 2);
    assert_eq!(undo.failed_count, 0);

    // 撤销后：文件回到源目录，目标目录空
    assert!(scan_root.path().join("a.txt").exists());
    assert!(scan_root.path().join("b.txt").exists());
    assert!(!target_dir.path().join("a.txt").exists());

    // files 表路径回写 + 日志标记 undone
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let rec = FileRepo::get_by_id(guard.conn(), &exec.results[0].file_id)
        .map_err(|e| e.to_string())?
        .ok_or("files 表应有记录")?;
    assert!(
        rec.path
            .starts_with(scan_root.path().to_string_lossy().as_ref()),
        "撤销后 files.path 应回写源目录: {}",
        rec.path
    );
    let logs =
        OperationRepo::list_by_batch(guard.conn(), &exec.batch_id).map_err(|e| e.to_string())?;
    assert!(logs.iter().all(|l| l.status == "undone"));
    Ok(())
}

#[test]
fn test_undo_batch_copy_removes_copy() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (_, exec) = preview_then_execute(
        &state,
        scan_root.path(),
        &["orig.txt"],
        OperationType::Copy,
        Some(target_dir.path().to_string_lossy().to_string()),
        vec![],
    )?;
    assert_eq!(exec.summary.success, 1);
    assert!(target_dir.path().join("orig.txt").exists());

    let undo = undo_batch_inner(&exec.batch_id, &state)?;
    assert!(undo.success);
    assert_eq!(undo.undone_count, 1);

    // 副本已删除，源文件保留
    assert!(!target_dir.path().join("orig.txt").exists());
    assert!(scan_root.path().join("orig.txt").exists());
    Ok(())
}

#[test]
fn test_undo_batch_nonexistent_batch_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let result = undo_batch_inner("nonexistent-batch", &state);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::InvalidInput(_)));
    Ok(())
}

#[test]
fn test_undo_batch_delete_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    let trash_dir = tempfile::tempdir()?;
    let _guard = crate::services::trash::TEST_TRASH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::env::set_var("FILEMIND_TRASH_DIR", trash_dir.path());

    let (_, exec) = preview_then_execute(
        &state,
        scan_root.path(),
        &["del.txt"],
        OperationType::Delete,
        None,
        vec![],
    )?;
    assert_eq!(exec.summary.success, 1);

    let result = undo_batch_inner(&exec.batch_id, &state);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::Forbidden(_)));
    Ok(())
}

#[test]
fn test_delete_files_moves_all_to_trash() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    let trash_dir = tempfile::tempdir()?;
    let _guard = crate::services::trash::TEST_TRASH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::env::set_var("FILEMIND_TRASH_DIR", trash_dir.path());
    let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt", "b.txt"])?;

    let results = delete_files_inner(&ids, &state)?;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success && r.error.is_none()));

    // 磁盘原文件已消失（移入系统回收站）
    assert!(!scan_root.path().join("a.txt").exists());
    assert!(!scan_root.path().join("b.txt").exists());
    // 文件确实移入了回收站目录（而非物理删除）
    assert!(trash_dir.path().join("a.txt").exists());
    assert!(trash_dir.path().join("b.txt").exists());

    // DB 软删除：get_by_id 不返回软删除行 → 每条应查无记录
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    for id in &ids {
        let file = FileRepo::get_by_id(guard.conn(), id).map_err(|e| e.to_string())?;
        assert!(file.is_none(), "删除后记录应被软删除（id={id}）");
    }
    let count: i64 = guard
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM operations_log WHERE operation_type = 'delete'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    assert_eq!(count, 2, "应写入 2 条 delete 审计日志");
    Ok(())
}

#[test]
fn test_delete_files_partial_unknown_id_fails_others_succeed(
) -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    let trash_dir = tempfile::tempdir()?;
    let _guard = crate::services::trash::TEST_TRASH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::env::set_var("FILEMIND_TRASH_DIR", trash_dir.path());
    let mut ids = seed_files_for_preview(&state, scan_root.path(), &["keep.txt"])?;
    ids.push("missing-id".into());

    let results = delete_files_inner(&ids, &state)?;
    assert_eq!(results.len(), 2);
    assert!(results[0].success, "存在的文件应删除成功");
    assert!(results[0].error.is_none());
    assert!(!results[1].success, "未知 id 应返回失败");
    assert!(results[1].error.is_some());

    // 存在的文件已移走；未知 id 不产生副作用
    assert!(!scan_root.path().join("keep.txt").exists());
    Ok(())
}

#[test]
fn test_undo_batch_double_undo_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (_, exec) = preview_then_execute(
        &state,
        scan_root.path(),
        &["a.txt"],
        OperationType::Move,
        Some(target_dir.path().to_string_lossy().to_string()),
        vec![],
    )?;

    // 第一次撤销成功
    let undo1 = undo_batch_inner(&exec.batch_id, &state)?;
    assert!(undo1.success);

    // 第二次撤销被拒绝
    let undo2 = undo_batch_inner(&exec.batch_id, &state);
    assert!(undo2.is_err());
    assert!(matches!(undo2.unwrap_err(), AppError::Forbidden(_)));
    Ok(())
}
