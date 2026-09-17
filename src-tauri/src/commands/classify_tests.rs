//! `classify` 命令的单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `classify.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::db::models::FileRecord;
use crate::db::Database;
use crate::sidecar::SidecarManager;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

fn make_test_app_state(db_path: &std::path::Path) -> AppState {
    let db = Database::open(db_path).expect("打开测试 DB 失败");
    AppState {
        db: std::sync::Arc::new(Mutex::new(db)),
        sidecar_manager: Mutex::new(SidecarManager::new("/dev/null/sidecar-nonexistent".into())),
        sidecar_psk: Mutex::new(None),
        sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
        request_seq: AtomicU64::new(0),
        sidecar_restart_count: AtomicU64::new(0),
        sidecar_status: Mutex::new(crate::SidecarStatus::Starting),
    }
}

fn seed_file(state: &AppState, root: &std::path::Path, name: &str) -> String {
    std::fs::write(root.join(name), b"x").unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let rec = FileRecord {
        id: id.clone(),
        path: root.join(name).to_string_lossy().to_string(),
        file_name: name.to_string(),
        file_size: 1,
        content_hash: None,
        category: None,
        is_deleted: false,
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
        mtime: None,
    };
    let guard = state.db.lock().unwrap();
    FileRepo::upsert_batch(guard.conn(), &[rec]).unwrap();
    id
}

#[tokio::test]
async fn test_classify_preview_returns_plan_and_stats() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 内置分类种子：启发式「图片」目标可用
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        CategoryRepo::seed_builtin_categories(db.conn()).map_err(|e| e.to_string())?;
    }

    let id = seed_file(&state, root.path(), "photo.png");
    let preview =
        classify_preview_inner(&state, &[id], root.path().to_str().ok_or("路径非 UTF-8")?).await?;

    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].category_name.as_deref(), Some("图片"));
    assert_eq!(preview.items[0].rule_source, "heuristic");
    assert_eq!(preview.stats.categorized, 1);
    assert_eq!(preview.stats.pending, 0);
    assert_eq!(preview.batch_id.len(), 36);
    Ok(())
}

/// sidecar 不可用（PSK 为 None）时，待确认项保持 pending，预览不失败。
#[tokio::test]
async fn test_classify_preview_pending_kept_when_sidecar_unavailable(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        CategoryRepo::seed_builtin_categories(db.conn()).map_err(|e| e.to_string())?;
    }

    // .xyz 无启发式映射、无规则 → 待确认；PSK=None → LLM 兜底降级
    let id = seed_file(&state, root.path(), "mystery.xyz");
    let preview =
        classify_preview_inner(&state, &[id], root.path().to_str().ok_or("路径非 UTF-8")?).await?;

    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].category_name, None);
    assert_eq!(preview.items[0].rule_source, classifier::PENDING_SOURCE);
    assert_eq!(preview.stats.pending, 1);
    Ok(())
}

#[tokio::test]
async fn test_classify_preview_empty_ids_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let result =
        classify_preview_inner(&state, &[], root.path().to_str().ok_or("路径非 UTF-8")?).await;
    assert!(matches!(result, Err(AppError::InvalidInput(_))));
    Ok(())
}

#[tokio::test]
async fn test_classify_preview_unsafe_scan_root_rejected() -> Result<(), Box<dyn std::error::Error>>
{
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let result = classify_preview_inner(&state, &["f1".to_string()], "/System").await;
    assert!(matches!(result, Err(AppError::UnsafePath(_))));
    Ok(())
}

/// 收纳根 = 扫描根同级的 `<扫描根名>_已分类`；目录不存在时直接采用该名。
#[test]
fn test_unique_output_root_sibling_used_when_free() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let out = unique_output_root(root.path())?;
    let expected = classifier::sibling_output_root(root.path())?;
    assert_eq!(out, expected);
    assert!(!out.exists(), "临时目录同级应无收纳目录，此处仅校验命名");
    Ok(())
}

/// 同级收纳目录名被普通文件占用 → 明确报错（而非悄悄落到别处）。
#[test]
fn test_unique_output_root_rejects_occupied_by_file() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let occupied = classifier::sibling_output_root(root.path())?;
    if let Some(parent) = occupied.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&occupied, b"occupied")?;

    let result = unique_output_root(root.path());
    assert!(matches!(result, Err(AppError::InvalidInput(_))));
    Ok(())
}

/// 收纳目录已存在（同批/分批整理）→ 复用同名目录，不报错不递增。
#[test]
fn test_unique_output_root_reuses_existing_dir() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let out = classifier::sibling_output_root(root.path())?;
    std::fs::create_dir_all(&out)?;

    let resolved = unique_output_root(root.path())?;
    assert_eq!(resolved, out);
    Ok(())
}
