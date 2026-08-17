use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{FileRepo, FileSearch, SearchResult};
use crate::error::AppError;
use crate::{AppState, FileInfo};

const MAX_PAGE_SIZE: i64 = 500;

#[tauri::command]
#[specta::specta]
pub async fn list_files(
    state: State<'_, AppState>,
    category: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<FileListResponse, String> {
    let page = page.unwrap_or(0) as i64;
    let page_size = page_size.unwrap_or(50) as i64;
    let limit = page_size.min(MAX_PAGE_SIZE).max(1);
    let offset = page * limit;

    let db = lock_db(&state.db)?;
    let files = FileRepo::list(db.conn(), category.as_deref(), offset, limit)
        .map_err(|e| format!("DB-U-001:数据读取失败，请重启应用 ({e})"))?;
    let total = FileRepo::count(db.conn(), category.as_deref())
        .map_err(|e| format!("DB-U-001:数据读取失败，请重启应用 ({e})"))?;

    let file_infos: Vec<FileInfo> = files.into_iter().map(FileInfo::from).collect();
    Ok(FileListResponse {
        files: file_infos,
        total,
        page,
        page_size: limit,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn search_files(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let search_limit = limit
        .unwrap_or(50) as i64
        .min(MAX_PAGE_SIZE)
        .max(1);

    let db = lock_db(&state.db)?;
    let results = FileSearch::search(db.conn(), &query, search_limit)
        .map_err(|e| format!("DB-U-001:搜索失败 ({e})"))?;

    Ok(results)
}

#[tauri::command]
#[specta::specta]
pub async fn search_by_filename(
    state: State<'_, AppState>,
    pattern: String,
    limit: Option<u32>,
) -> Result<Vec<FileInfo>, String> {
    let search_limit = limit
        .unwrap_or(50) as i64
        .min(MAX_PAGE_SIZE)
        .max(1);

    let db = lock_db(&state.db)?;
    let files = FileSearch::search_by_filename(db.conn(), &pattern, search_limit)
        .map_err(|e| format!("DB-U-001:搜索失败 ({e})"))?;

    let file_infos: Vec<FileInfo> = files.into_iter().map(FileInfo::from).collect();
    Ok(file_infos)
}

#[tauri::command]
#[specta::specta]
pub async fn get_file_stats(
    state: State<'_, AppState>,
) -> Result<FileStats, String> {
    let db = lock_db(&state.db)?;

    let total: i64 = db.conn()
        .query_row("SELECT COUNT(*) FROM files WHERE is_deleted = 0", [], |row| row.get(0))
        .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

    let categorized: i64 = db.conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND category IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

    let duplicates: i64 = db.conn()
        .query_row(
            "SELECT COUNT(*) - COUNT(DISTINCT content_hash) FROM files
             WHERE is_deleted = 0 AND content_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

    let total_size: i64 = db.conn()
        .query_row(
            "SELECT COALESCE(SUM(file_size), 0) FROM files WHERE is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

    Ok(FileStats {
        total_files: total,
        categorized_files: categorized,
        uncategorized_files: total - categorized,
        duplicate_groups: duplicates,
        total_size_bytes: total_size,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn update_file_category(
    state: State<'_, AppState>,
    id: String,
    category: String,
) -> Result<(), String> {
    let db = lock_db(&state.db)?;
    FileRepo::update_category(db.conn(), &id, &category)
        .map_err(|e| match e {
            AppError::Database(rusqlite::Error::QueryReturnedNoRows) => {
                "FILE-E-001:文件不存在".to_string()
            }
            _ => format!("DB-U-001:更新失败 ({e})"),
        })?;
    Ok(())
}

fn lock_db(db: &Mutex<crate::db::Database>) -> Result<std::sync::MutexGuard<'_, crate::db::Database>, String> {
    db.lock().map_err(|e| {
        log::error!("DB lock poisoned: {e}");
        "DB-U-001:数据读取失败，请重启应用".to_string()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileListResponse {
    pub files: Vec<FileInfo>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileStats {
    pub total_files: i64,
    pub categorized_files: i64,
    pub uncategorized_files: i64,
    pub duplicate_groups: i64,
    pub total_size_bytes: i64,
}
