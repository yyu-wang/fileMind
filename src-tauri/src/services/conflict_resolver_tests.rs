//! `conflict_resolver` 单元测试：4 个策略 + 文件名拆分 + 递增找名。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::conflict_resolver::{
    resolve, resolve_with_strategy, split_file_name, ConflictStrategy, ConflictType, PlanStatus,
};
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn make_file(dir: &Path, name: &str) -> std::io::Result<()> {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = File::create(&path)?;
    f.write_all(b"x")?;
    Ok(())
}

fn tmp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录失败")
}

// ============================================================================
// Rename 策略
// ============================================================================

#[test]
fn test_rename_strategy_target_not_exists() {
    let dir = tmp_dir();
    let target_dir = dir.path();

    let (new_path, status, conflict) = resolve_with_strategy(
        "report.pdf",
        target_dir,
        &target_dir.join("report.pdf"),
        false, // 目标不存在
        ConflictStrategy::Rename,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), target_dir.join("report.pdf"));
}

#[test]
fn test_rename_strategy_target_exists_first_increment() {
    let dir = tmp_dir();
    make_file(dir.path(), "report.pdf").unwrap();

    let (new_path, status, conflict) = resolve_with_strategy(
        "report.pdf",
        dir.path(),
        &dir.path().join("report.pdf"),
        true, // 目标已存在
        ConflictStrategy::Rename,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("report_1.pdf"));
}

#[test]
fn test_rename_strategy_target_exists_recursive_increment() {
    let dir = tmp_dir();
    // 目标已存在 report.pdf 和 report_1.pdf → 应跳到 report_2.pdf
    make_file(dir.path(), "report.pdf").unwrap();
    make_file(dir.path(), "report_1.pdf").unwrap();

    let (new_path, status, _conflict) = resolve_with_strategy(
        "report.pdf",
        dir.path(),
        &dir.path().join("report.pdf"),
        true,
        ConflictStrategy::Rename,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(new_path.unwrap(), dir.path().join("report_2.pdf"));
}

// ============================================================================
// Overwrite 策略
// ============================================================================

#[test]
fn test_overwrite_strategy_target_not_exists() {
    let dir = tmp_dir();
    let (new_path, status, conflict) = resolve_with_strategy(
        "doc.txt",
        dir.path(),
        &dir.path().join("doc.txt"),
        false,
        ConflictStrategy::Overwrite,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("doc.txt"));
}

#[test]
fn test_overwrite_strategy_target_exists_keeps_name() {
    let dir = tmp_dir();
    make_file(dir.path(), "doc.txt").unwrap();

    let (new_path, status, conflict) = resolve_with_strategy(
        "doc.txt",
        dir.path(),
        &dir.path().join("doc.txt"),
        true,
        ConflictStrategy::Overwrite,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("doc.txt"));
}

// ============================================================================
// Skip 策略
// ============================================================================

#[test]
fn test_skip_strategy_target_not_exists() {
    let dir = tmp_dir();
    let (new_path, status, conflict) = resolve_with_strategy(
        "data.csv",
        dir.path(),
        &dir.path().join("data.csv"),
        false,
        ConflictStrategy::Skip,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("data.csv"));
}

#[test]
fn test_skip_strategy_target_exists_returns_conflict() {
    let dir = tmp_dir();
    make_file(dir.path(), "data.csv").unwrap();

    let (new_path, status, conflict) = resolve_with_strategy(
        "data.csv",
        dir.path(),
        &dir.path().join("data.csv"),
        true,
        ConflictStrategy::Skip,
    );

    assert_eq!(status, PlanStatus::Conflict);
    assert_eq!(conflict, Some(ConflictType::SameName));
    assert!(new_path.is_none(), "Skip 策略冲突时 new_path 应为 None");
}

// ============================================================================
// KeepBoth 策略
// ============================================================================

#[test]
fn test_keep_both_strategy_target_not_exists() {
    let dir = tmp_dir();
    let (new_path, status, conflict) = resolve_with_strategy(
        "img.png",
        dir.path(),
        &dir.path().join("img.png"),
        false,
        ConflictStrategy::KeepBoth,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("img.png"));
}

#[test]
fn test_keep_both_strategy_target_exists_appends_copy_suffix() {
    let dir = tmp_dir();
    make_file(dir.path(), "img.png").unwrap();

    let (new_path, status, conflict) = resolve_with_strategy(
        "img.png",
        dir.path(),
        &dir.path().join("img.png"),
        true,
        ConflictStrategy::KeepBoth,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(conflict, None);
    assert_eq!(new_path.unwrap(), dir.path().join("img_copy.png"));
}

#[test]
fn test_keep_both_strategy_copy_already_exists() {
    let dir = tmp_dir();
    make_file(dir.path(), "img.png").unwrap();
    make_file(dir.path(), "img_copy.png").unwrap();

    let (new_path, status, _conflict) = resolve_with_strategy(
        "img.png",
        dir.path(),
        &dir.path().join("img.png"),
        true,
        ConflictStrategy::KeepBoth,
    );

    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(new_path.unwrap(), dir.path().join("img_copy_1.png"));
}

// ============================================================================
// 文件名拆分辅助函数
// ============================================================================

#[test]
fn test_split_file_name_with_extension() {
    assert_eq!(split_file_name("report.pdf"), ("report", "pdf"));
    assert_eq!(split_file_name("a.b.c.d"), ("a.b.c", "d"));
}

#[test]
fn test_split_file_name_without_extension() {
    assert_eq!(split_file_name("noext"), ("noext", ""));
}

#[test]
fn test_split_file_name_hidden_file() {
    // .gitignore → stem="" ext="gitignore"（隐藏文件）
    assert_eq!(split_file_name(".gitignore"), ("", "gitignore"));
}

#[test]
fn test_split_file_name_empty_name() {
    assert_eq!(split_file_name(""), ("", ""));
}

// ============================================================================
// 公开入口 resolve（含文件系统 exists 检查）
// ============================================================================

#[test]
fn test_resolve_public_api_rename() {
    let dir = tmp_dir();
    make_file(dir.path(), "existing.pdf").unwrap();

    let (new_path, status, _conflict) = resolve(
        "file.pdf",
        std::path::Path::new("/tmp/orig.pdf"),
        dir.path(),
        ConflictStrategy::Rename,
    );

    // file.pdf 不冲突 → 直接用原名
    assert_eq!(status, PlanStatus::Ok);
    assert_eq!(new_path.unwrap(), dir.path().join("file.pdf"));
}
