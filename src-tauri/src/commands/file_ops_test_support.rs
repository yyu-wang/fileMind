//! `file_ops` 测试的公共夹具：临时文件/目录、最小 AppState、预览与冲突计划构造。
//!
//! 独立成模块的原因：这些 helper 被 4 个测试模块共用；函数为 `pub(super)`，
//! 供兄弟测试模块以 `use super::file_ops_test_support::…` 引用。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::*;
use crate::db::Database;
use crate::sidecar::SidecarManager;
use crate::AppState;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

pub(super) fn create_temp_file(
    dir: &Path,
    name: &str,
    content: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path)?;
    file.write_all(content.as_bytes())?;
    Ok(path)
}

/// 构造最小可用 AppState（DB 指向临时 DB，Sidecar/PSK 用占位）。
pub(super) fn make_test_app_state(db_path: &std::path::Path) -> AppState {
    let db = Database::open(db_path).expect("打开测试 DB 失败");
    AppState {
        db: std::sync::Arc::new(std::sync::Mutex::new(db)),
        sidecar_manager: Mutex::new(SidecarManager::new("/dev/null/sidecar-nonexistent".into())),
        sidecar_psk: Mutex::new(None),
        sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
        request_seq: AtomicU64::new(0),
        sidecar_restart_count: AtomicU64::new(0),
        sidecar_status: Mutex::new(crate::SidecarStatus::Starting),
    }
}

/// 往临时 `AppState` 的 `SQLite` 里塞多个文件记录，返回它们的 ID。
///
/// 内部把 `PoisonError<MutexGuard<Database>>` 和 `rusqlite::Error` 转字符串后
/// 包成 `Box<dyn Error>`，避开 `MutexGuard` 的非 `'static` 借用问题
/// （`Database` 含 `RefCell`，不满足 `Sync`）。
pub(super) fn seed_files_for_preview(
    state: &AppState,
    scan_root: &Path,
    names: &[&str],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut ids = Vec::new();
    let mut records = Vec::new();
    for name in names {
        let path = scan_root.join(name);
        std::fs::write(&path, b"x")?;
        ids.push(uuid::Uuid::new_v4().to_string());
        records.push(FileRecord {
            id: ids.last().unwrap().clone(),
            path: path.to_string_lossy().to_string(),
            file_name: (*name).to_string(),
            file_size: 1,
            content_hash: Some("dummy".into()),
            category: None,
            is_deleted: false,
            created_at: "2025-01-01".into(),
            updated_at: "2025-01-01".into(),
            mtime: None,
        });
    }
    let guard = state
        .db
        .lock()
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
    FileRepo::upsert_batch(guard.conn(), &records)
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
    drop(guard);
    Ok(ids)
}

/// 辅助：从 preview 拿 plan 后调 execute，返回 `ExecuteResponse`。
pub(super) fn preview_then_execute(
    state: &AppState,
    scan_root: &Path,
    names: &[&str],
    operation: OperationType,
    target_dir: Option<String>,
    exclude: Vec<String>,
) -> Result<(PreviewResponse, ExecuteResponse), Box<dyn std::error::Error>> {
    let ids = seed_files_for_preview(state, scan_root, names)?;
    let preview = preview_operations_inner(
        PreviewRequest {
            file_ids: ids.clone(),
            operation,
            target_dir,
            conflict_strategy: Some(ConflictStrategy::Rename),
        },
        state,
    )?;
    let execute = execute_operations_inner(
        ExecuteRequest {
            batch_id: preview.batch_id.clone(),
            plan: preview.plan.clone(),
            exclude_file_ids: exclude,
            resolve_conflicts: false,
        },
        state,
    )?;
    Ok((preview, execute))
}

/// 构造冲突项 plan：目标目录已有同名文件 → status=Conflict。
pub(super) fn make_conflict_plan(
    state: &AppState,
    scan_root: &Path,
    target_dir: &Path,
    name: &str,
) -> Result<(String, Vec<PlanItem>), Box<dyn std::error::Error>> {
    // 目标目录已存在同名文件 → 构造 Conflict 项
    std::fs::write(target_dir.join(name), b"existing")?;
    std::fs::write(scan_root.join(name), b"src")?;
    let ids = seed_files_for_preview(state, scan_root, &[name])?;
    let plan = vec![PlanItem {
        file_id: ids[0].clone(),
        file_name: name.to_string(),
        original_path: scan_root.join(name).to_string_lossy().to_string(),
        new_path: Some(target_dir.join(name).to_string_lossy().to_string()),
        operation: OperationType::Move,
        status: PlanStatus::Conflict,
        conflict_type: Some(ConflictType::SameName),
    }];
    Ok((ids[0].clone(), plan))
}
