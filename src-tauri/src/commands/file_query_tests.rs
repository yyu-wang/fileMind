//! `file_query` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests.rs`）。

use super::*;
use crate::db::models::FileRecord;
use crate::db::Database;
use crate::sidecar::SidecarManager;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

/// 构造最小可用 AppState（DB 指向临时 DB，Sidecar/PSK 用占位）。
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

/// 往临时 DB 塞若干文件记录（`updated_at` 按序号递增），返回它们的路径。
fn seed_files(
    state: &AppState,
    count: usize,
    category: Option<&str>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut records = Vec::new();
    let mut paths = Vec::new();
    for i in 0..count {
        let path = format!("/tmp/seed/f{i:03}.txt");
        let rec = FileRecord {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            file_name: format!("f{i:03}.txt"),
            file_size: i64::try_from(i).unwrap_or(0),
            content_hash: Some(format!("hash-{i}")),
            category: category.map(str::to_string),
            is_deleted: false,
            created_at: format!("2026-08-{i:02} 00:00:00"),
            updated_at: format!("2026-08-{i:02} 00:00:00"),
            mtime: None,
        };
        paths.push(path);
        records.push(rec);
    }
    let guard = state
        .db
        .lock()
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
    FileRepo::upsert_batch(guard.conn(), &records)
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
    drop(guard);
    Ok(paths)
}

#[test]
fn test_list_all_returns_everything_sorted_desc() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    seed_files(&state, 5, None)?;

    // upsert_batch 会把 updated_at 覆盖为 now()，这里手工写入递增时间戳测排序
    {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        for i in 0..5 {
            guard
                .conn()
                .execute(
                    "UPDATE files SET updated_at = ?1 WHERE file_name = ?2",
                    rusqlite::params![format!("2026-08-{i:02} 00:00:00"), format!("f{i:03}.txt")],
                )
                .map_err(|e| e.to_string())?;
        }
    }

    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let files = list_all_from_conn(guard.conn(), None).map_err(|e| e.to_string())?;
    drop(guard);

    assert_eq!(files.len(), 5);
    // updated_at DESC：f004（2026-08-04）在最前 … f000（2026-08-00）在最后
    assert_eq!(files[0].file_name, "f004.txt");
    assert_eq!(files[4].file_name, "f000.txt");
    Ok(())
}

#[test]
fn test_list_all_filters_by_category() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    seed_files(&state, 3, Some("财务"))?;
    seed_files(&state, 2, Some("市场"))?;

    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let financial = list_all_from_conn(guard.conn(), Some("财务")).map_err(|e| e.to_string())?;
    drop(guard);

    assert_eq!(financial.len(), 3);
    assert!(financial
        .iter()
        .all(|f| f.category.as_deref() == Some("财务")));
    Ok(())
}

#[test]
fn test_list_all_excludes_soft_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());
    let paths = seed_files(&state, 2, None)?;

    // 软删一条（软删前先取 id，软删后 get_by_path 按 is_deleted=0 查不到）
    {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let rec = FileRepo::get_by_path(guard.conn(), &paths[0])
            .map_err(|e| e.to_string())?
            .ok_or("记录应存在")?;
        FileRepo::soft_delete(guard.conn(), &rec.id).map_err(|e| e.to_string())?;
    }

    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let files = list_all_from_conn(guard.conn(), None).map_err(|e| e.to_string())?;
    drop(guard);

    assert_eq!(files.len(), 1, "软删除文件不应出现在全量列表");
    Ok(())
}
