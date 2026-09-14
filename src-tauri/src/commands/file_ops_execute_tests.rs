//! 执行相关测试：`execute_operations` 的移动/复制/删除/排除项/失败留痕与冲突解决。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::file_ops_test_support::{
    make_conflict_plan, make_test_app_state, preview_then_execute, seed_files_for_preview,
};
use super::*;

#[test]
fn test_execute_operations_move_full_flow() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (_, exec) = preview_then_execute(
        &state,
        scan_root.path(),
        &["a.txt", "b.txt", "c.txt"],
        OperationType::Move,
        Some(target_dir.path().to_string_lossy().to_string()),
        vec![],
    )?;

    // 全部成功
    assert_eq!(exec.summary.total, 3);
    assert_eq!(exec.summary.success, 3);
    assert_eq!(exec.summary.failed, 0);
    assert_eq!(exec.summary.skipped, 0);

    // SQLite files 表路径已更新
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    for r in &exec.results {
        let file = FileRepo::get_by_id(guard.conn(), &r.file_id).map_err(|e| e.to_string())?;
        assert!(file.is_some(), "files 表应有记录");
        let f = file.unwrap();
        assert!(
            f.path
                .starts_with(target_dir.path().to_string_lossy().as_ref()),
            "files.path 应已更新到目标目录: {}",
            f.path
        );
        assert!(!f.is_deleted, "Move 后不应软删除");
    }

    // operations_log 有 3 条 done 记录
    let logs =
        OperationRepo::list_by_batch(guard.conn(), &exec.batch_id).map_err(|e| e.to_string())?;
    assert_eq!(logs.len(), 3);
    assert!(logs.iter().all(|l| l.status == "done"));
    assert!(logs.iter().all(|l| l.batch_id == exec.batch_id));
    Ok(())
}

#[test]
fn test_execute_operations_with_exclude() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 先 preview 拿到 3 个 plan
    let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt", "b.txt", "c.txt"])?;
    let preview = preview_operations_inner(
        PreviewRequest {
            file_ids: ids.clone(),
            operation: OperationType::Move,
            target_dir: Some(target_dir.path().to_string_lossy().to_string()),
            conflict_strategy: Some(ConflictStrategy::Rename),
        },
        &state,
    )?;

    // 排除第 2 个文件
    let excluded_id = ids[1].clone();
    let exec = execute_operations_inner(
        ExecuteRequest {
            batch_id: preview.batch_id.clone(),
            plan: preview.plan.clone(),
            exclude_file_ids: vec![excluded_id.clone()],
            resolve_conflicts: false,
        },
        &state,
    )?;

    assert_eq!(exec.summary.total, 3);
    assert_eq!(exec.summary.success, 2);
    assert_eq!(exec.summary.skipped, 1);
    assert_eq!(exec.summary.failed, 0);

    // 排除项不应出现在 results 里
    assert!(
        !exec.results.iter().any(|r| r.file_id == excluded_id),
        "排除项不应在 results 中"
    );

    // operations_log 应该只有 2 条（排除项没写日志）
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let logs =
        OperationRepo::list_by_batch(guard.conn(), &exec.batch_id).map_err(|e| e.to_string())?;
    assert_eq!(logs.len(), 2);
    Ok(())
}

#[test]
fn test_execute_operations_delete_soft_deletes() -> Result<(), Box<dyn std::error::Error>> {
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
        &["del1.txt", "del2.txt"],
        OperationType::Delete,
        None,
        vec![],
    )?;

    assert_eq!(exec.summary.success, 2);

    // files 表记录应被软删除
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    for r in &exec.results {
        // get_by_id 只返回未删除记录，软删除后应返回 None
        let file = FileRepo::get_by_id(guard.conn(), &r.file_id).map_err(|e| e.to_string())?;
        assert!(file.is_none(), "Delete 后记录应被软删除");
    }

    // operations_log 有 2 条 done 记录，operation_type='delete'
    let logs =
        OperationRepo::list_by_batch(guard.conn(), &exec.batch_id).map_err(|e| e.to_string())?;
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|l| l.operation_type == "delete"));
    assert!(logs.iter().all(|l| l.target_path.is_empty()));
    Ok(())
}

#[test]
fn test_execute_operations_copy_does_not_modify_files_table(
) -> Result<(), Box<dyn std::error::Error>> {
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

    // Copy 后 files 表原记录路径不变
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let file =
        FileRepo::get_by_id(guard.conn(), &exec.results[0].file_id).map_err(|e| e.to_string())?;
    let f = file.expect("files 表应有原记录");
    assert!(
        f.path
            .starts_with(scan_root.path().to_string_lossy().as_ref()),
        "Copy 后源记录路径不应变化"
    );
    assert!(!f.is_deleted);
    Ok(())
}

#[test]
fn test_execute_operations_failed_item_still_writes_log() -> Result<(), Box<dyn std::error::Error>>
{
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?; // 真实存在的目录，让 preview 通过
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 先 preview 拿到 plan
    let ids = seed_files_for_preview(&state, scan_root.path(), &["real.txt"])?;
    let preview = preview_operations_inner(
        PreviewRequest {
            file_ids: ids,
            operation: OperationType::Move,
            target_dir: Some(target_dir.path().to_string_lossy().to_string()),
            conflict_strategy: Some(ConflictStrategy::Rename),
        },
        &state,
    )?;

    // 把源文件删除，让 execute 阶段失败（preview 已通过）
    std::fs::remove_file(scan_root.path().join("real.txt"))?;

    let exec = execute_operations_inner(
        ExecuteRequest {
            batch_id: preview.batch_id.clone(),
            plan: preview.plan.clone(),
            exclude_file_ids: vec![],
            resolve_conflicts: false,
        },
        &state,
    )?;

    // 失败也算入 total，但不计入 skipped
    assert_eq!(exec.summary.total, 1);
    assert_eq!(exec.summary.success, 0);
    assert_eq!(exec.summary.failed, 1);

    // 失败的项也写日志，status=failed
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let logs =
        OperationRepo::list_by_batch(guard.conn(), &exec.batch_id).map_err(|e| e.to_string())?;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
    Ok(())
}

#[test]
fn test_execute_resolve_conflicts_renames_target() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (file_id, plan) = make_conflict_plan(&state, scan_root.path(), target_dir.path(), "a.txt")?;
    let exec = execute_operations_inner(
        ExecuteRequest {
            batch_id: "b-resolve".into(),
            plan,
            exclude_file_ids: vec![],
            resolve_conflicts: true,
        },
        &state,
    )?;

    // 冲突项经 Rename 解析后执行成功：生成 a_1.txt，原同名文件保留
    assert_eq!(exec.summary.total, 1);
    assert_eq!(exec.summary.success, 1);
    assert_eq!(exec.summary.skipped, 0);
    assert!(target_dir.path().join("a_1.txt").exists(), "应生成 a_1.txt");
    assert!(target_dir.path().join("a.txt").exists(), "原同名文件应保留");
    assert!(!scan_root.path().join("a.txt").exists(), "源文件应已移走");

    // files 表路径回写到 a_1.txt
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let rec = FileRepo::get_by_id(guard.conn(), &file_id)
        .map_err(|e| e.to_string())?
        .ok_or("文件应存在")?;
    assert!(
        rec.path.ends_with("a_1.txt"),
        "files.path 应回写为 a_1.txt: {}",
        rec.path
    );
    Ok(())
}

#[test]
fn test_execute_skips_conflicts_when_not_resolving() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let target_dir = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let (_, plan) = make_conflict_plan(&state, scan_root.path(), target_dir.path(), "a.txt")?;
    let exec = execute_operations_inner(
        ExecuteRequest {
            batch_id: "b-skip".into(),
            plan,
            exclude_file_ids: vec![],
            resolve_conflicts: false,
        },
        &state,
    )?;

    // 不解析冲突：跳过，源文件不动
    assert_eq!(exec.summary.total, 1);
    assert_eq!(exec.summary.success, 0);
    assert_eq!(exec.summary.skipped, 1);
    assert!(scan_root.path().join("a.txt").exists());
    assert!(!target_dir.path().join("a_1.txt").exists());
    Ok(())
}
