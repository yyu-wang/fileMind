//! 预览相关测试：`preview_operations` 的目标路径/冲突/Rename 策略与入参校验。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::test_support::{create_temp_file, make_test_app_state, seed_files_for_preview};
use super::*;

// 拆分后按需显式引入：内部实现来自子模块，外部名字原先靠 `use super::*` 取到
use super::preview::preview_operations_inner;
use crate::error::AppError;
use crate::services::conflict_resolver::{ConflictStrategy, PlanStatus};

#[test]
fn test_preview_operations_move_rename_strategy() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt", "b.txt", "c.txt"])?;
    assert_eq!(ids.len(), 3);

    let target_dir = tempfile::tempdir()?;
    let req = PreviewRequest {
        file_ids: ids.clone(),
        operation: OperationType::Move,
        target_dir: Some(target_dir.path().to_string_lossy().to_string()),
        conflict_strategy: Some(ConflictStrategy::Rename),
    };

    let resp = preview_operations_inner(req, &state)?;

    // 全部 status=Ok
    assert_eq!(resp.summary.total, 3);
    assert_eq!(resp.summary.ok, 3);
    assert_eq!(resp.summary.conflict, 0);
    assert_eq!(resp.summary.error, 0);

    // new_path = target_dir/{原文件名}
    for item in &resp.plan {
        assert_eq!(item.status, PlanStatus::Ok);
        let new_path = item.new_path.as_ref().expect("Move 应有 new_path");
        assert!(new_path.starts_with(target_dir.path().to_string_lossy().as_ref()));
    }

    // batch_id 是 36 字符 uuid（带连字符，与 uuid::Uuid::new_v4().to_string() 一致）
    assert_eq!(resp.batch_id.len(), 36);
    // 形如 xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
    assert_eq!(resp.batch_id.matches('-').count(), 4);
    assert!(resp
        .batch_id
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-'));
    Ok(())
}

#[test]
fn test_preview_operations_move_with_conflict_rename() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 源文件
    let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt"])?;

    // 目标目录已存在同名文件 a.txt → 应触发 rename 为 a_1.txt
    let target_dir = tempfile::tempdir()?;
    create_temp_file(target_dir.path(), "a.txt", "existing")?;

    let req = PreviewRequest {
        file_ids: ids,
        operation: OperationType::Move,
        target_dir: Some(target_dir.path().to_string_lossy().to_string()),
        conflict_strategy: Some(ConflictStrategy::Rename),
    };
    let resp = preview_operations_inner(req, &state)?;

    assert_eq!(resp.summary.ok, 1);
    let new_path = resp.plan[0].new_path.as_ref().unwrap();
    assert!(
        new_path.ends_with("a_1.txt"),
        "Rename 策略下冲突应生成 _1 后缀: {new_path}"
    );
    Ok(())
}

#[test]
fn test_preview_operations_delete_no_target_dir() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let ids = seed_files_for_preview(&state, scan_root.path(), &["x.txt", "y.txt"])?;

    let req = PreviewRequest {
        file_ids: ids,
        operation: OperationType::Delete,
        target_dir: None,
        conflict_strategy: None,
    };
    let resp = preview_operations_inner(req, &state)?;

    assert_eq!(resp.summary.total, 2);
    assert_eq!(resp.summary.ok, 2);
    for item in &resp.plan {
        assert!(item.new_path.is_none(), "Delete 操作 new_path 应为 None");
        assert_eq!(item.operation, OperationType::Delete);
    }
    Ok(())
}

#[test]
fn test_preview_operations_missing_target_dir_for_move() {
    let tmp_db = tempfile::NamedTempFile::new().unwrap();
    let state = make_test_app_state(tmp_db.path());

    let req = PreviewRequest {
        file_ids: vec!["fake-id".into()],
        operation: OperationType::Move,
        target_dir: None,
        conflict_strategy: None,
    };
    let result = preview_operations_inner(req, &state);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::InvalidInput(_)),
        "期望 InvalidInput"
    );
}

#[test]
fn test_preview_operations_empty_file_ids() {
    let tmp_db = tempfile::NamedTempFile::new().unwrap();
    let state = make_test_app_state(tmp_db.path());

    let req = PreviewRequest {
        file_ids: vec![],
        operation: OperationType::Delete,
        target_dir: None,
        conflict_strategy: None,
    };
    let result = preview_operations_inner(req, &state);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::InvalidInput(_)));
}

#[test]
fn test_preview_operations_unknown_file_id_silently_skipped(
) -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let real_ids = seed_files_for_preview(&state, scan_root.path(), &["real.txt"])?;
    let mut all_ids = real_ids.clone();
    all_ids.push("non-existent-uuid".into()); // 不存在的 ID

    let target_dir = tempfile::tempdir()?;
    let req = PreviewRequest {
        file_ids: all_ids,
        operation: OperationType::Move,
        target_dir: Some(target_dir.path().to_string_lossy().to_string()),
        conflict_strategy: None, // 默认 Rename
    };

    let resp = preview_operations_inner(req, &state)?;
    // 不存在的 ID 静默跳过 → plan 只有 1 项（真实文件）
    assert_eq!(resp.plan.len(), 1);
    assert_eq!(resp.summary.total, 1);
    assert_eq!(resp.summary.ok, 1);
    Ok(())
}

#[test]
fn test_preview_operations_batch_id_is_uuid_format() -> Result<(), Box<dyn std::error::Error>> {
    let scan_root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let ids = seed_files_for_preview(&state, scan_root.path(), &["only.txt"])?;

    let req = PreviewRequest {
        file_ids: ids,
        operation: OperationType::Delete,
        target_dir: None,
        conflict_strategy: None,
    };
    let resp = preview_operations_inner(req, &state)?;

    // uuid4 → 36 字符（带连字符）
    assert_eq!(resp.batch_id.len(), 36);
    assert_eq!(resp.batch_id.matches('-').count(), 4);
    assert!(resp
        .batch_id
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-'));
    Ok(())
}
