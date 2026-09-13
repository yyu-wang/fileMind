//! `operation_executor` 单元测试：Move/Copy/Delete/非 Ok 状态/失败分支。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::operation_executor::execute_plan_item;
use crate::commands::file_ops::{OperationType, PlanItem};
use crate::services::conflict_resolver::{ConflictType, PlanStatus};
use std::fs;
use std::io::Write;
use std::path::Path;

fn make_source_file(dir: &Path, name: &str, content: &str) -> std::io::Result<()> {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(&path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

fn make_plan_item(
    operation: OperationType,
    original_path: &str,
    new_path: Option<&str>,
    status: PlanStatus,
) -> PlanItem {
    let conflict_type = if status == PlanStatus::Conflict {
        Some(ConflictType::SameName)
    } else {
        None
    };
    PlanItem {
        file_id: "test-id".into(),
        file_name: Path::new(original_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        original_path: original_path.into(),
        new_path: new_path.map(String::from),
        operation,
        status,
        conflict_type,
    }
}

#[test]
fn test_execute_move_success() {
    let src_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    make_source_file(src_dir.path(), "a.txt", "hello").unwrap();
    let src = src_dir.path().join("a.txt");
    let target = target_dir.path().join("a.txt");

    let item = make_plan_item(
        OperationType::Move,
        src.to_str().unwrap(),
        Some(target.to_str().unwrap()),
        PlanStatus::Ok,
    );

    let (success, error, current_hash) = execute_plan_item(&item, Some("prev-hash".into()));

    assert!(success, "Move 应成功: error={error:?}");
    assert!(error.is_none());
    // Move 后内容未变，current_hash = prev_hash
    assert_eq!(current_hash.as_deref(), Some("prev-hash"));
    // 文件已移动：源不存在，目标存在
    assert!(!src.exists());
    assert!(target.exists());
}

#[test]
fn test_execute_copy_success() {
    let src_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    make_source_file(src_dir.path(), "orig.txt", "content").unwrap();
    let src = src_dir.path().join("orig.txt");
    let target = target_dir.path().join("orig.txt");

    let item = make_plan_item(
        OperationType::Copy,
        src.to_str().unwrap(),
        Some(target.to_str().unwrap()),
        PlanStatus::Ok,
    );

    let (success, error, current_hash) = execute_plan_item(&item, Some("hash-of-content".into()));

    assert!(success, "Copy 应成功: error={error:?}");
    assert!(error.is_none());
    assert_eq!(current_hash.as_deref(), Some("hash-of-content"));
    // Copy 后源和目标都存在
    assert!(src.exists());
    assert!(target.exists());
}

#[test]
fn test_execute_delete_success() {
    let src_dir = tempfile::tempdir().unwrap();
    make_source_file(src_dir.path(), "to_delete.txt", "x").unwrap();
    // 覆盖回收站目标目录：避免真实系统回收站（沙箱/并行污染），确定性断言
    let trash_dir = tempfile::tempdir().unwrap();
    let _guard = crate::services::trash::TEST_TRASH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::env::set_var("FILEMIND_TRASH_DIR", trash_dir.path());
    let src = src_dir.path().join("to_delete.txt");

    let item = make_plan_item(
        OperationType::Delete,
        src.to_str().unwrap(),
        None,
        PlanStatus::Ok,
    );

    let (success, error, current_hash) = execute_plan_item(&item, Some("some-hash".into()));

    assert!(success, "Delete 应成功: error={error:?}");
    assert!(error.is_none());
    // Delete 后 current_hash = None
    assert!(current_hash.is_none(), "Delete 后 current_hash 应为 None");
    // 文件已移入回收站目录（原位置消失、目标目录存在同名文件）
    assert!(!src.exists());
    assert!(trash_dir.path().join("to_delete.txt").exists());
}

#[test]
fn test_execute_non_ok_plan_returns_failure() {
    let item = make_plan_item(
        OperationType::Move,
        "/tmp/anywhere.txt",
        Some("/tmp/target.txt"),
        PlanStatus::Conflict,
    );

    let (success, error, current_hash) = execute_plan_item(&item, None);

    assert!(!success);
    assert!(error.unwrap().contains("非 Ok"));
    assert!(current_hash.is_none());
}

#[test]
fn test_execute_source_not_exists_returns_failure() {
    let item = make_plan_item(
        OperationType::Move,
        "/tmp/nonexistent-file-12345.txt",
        Some("/tmp/target.txt"),
        PlanStatus::Ok,
    );

    let (success, error, current_hash) = execute_plan_item(&item, None);

    assert!(!success, "源文件不存在应失败");
    assert!(error.is_some());
    assert!(current_hash.is_none());
}

#[test]
fn test_execute_move_auto_create_parent_dirs() {
    let src_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    make_source_file(src_dir.path(), "deep.txt", "x").unwrap();
    let src = src_dir.path().join("deep.txt");
    // 深层目标目录
    let target = target_dir.path().join("a/b/c/d/deep.txt");

    let item = make_plan_item(
        OperationType::Move,
        src.to_str().unwrap(),
        Some(target.to_str().unwrap()),
        PlanStatus::Ok,
    );

    let (success, error, _) = execute_plan_item(&item, None);

    assert!(success, "Move 应自动创建父目录: error={error:?}");
    assert!(target.exists());
}

#[test]
fn test_execute_copy_auto_create_parent_dirs() {
    let src_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    make_source_file(src_dir.path(), "file.txt", "content").unwrap();
    let src = src_dir.path().join("file.txt");
    let target = target_dir.path().join("x/y/z/file.txt");

    let item = make_plan_item(
        OperationType::Copy,
        src.to_str().unwrap(),
        Some(target.to_str().unwrap()),
        PlanStatus::Ok,
    );

    let (success, error, _) = execute_plan_item(&item, None);

    assert!(success, "Copy 应自动创建父目录: error={error:?}");
    assert!(target.exists());
    assert!(src.exists(), "Copy 后源仍应存在");
}
