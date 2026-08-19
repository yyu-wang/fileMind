//! 文件查询命令：分页列表、全文搜索、文件名搜索与统计。
//!
//! 所有命令访问 SQLite（阻塞 IO），通过 `#[tauri::command(async)]`
//! 声明为线程池执行；数据库锁的作用域收窄到查询完成即释放。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{
    FileRepo, FileSearch, OperationBatchSummary, OperationLog, OperationRepo, SearchResult,
};
use crate::error::AppError;
use crate::services::undo_window;
use crate::{AppState, FileInfo};

const MAX_PAGE_SIZE: i64 = 500;

/// 分页获取文件列表，可按分类过滤。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn list_files(
    state: State<'_, AppState>,
    category: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<FileListResponse, String> {
    let page = i64::from(page.unwrap_or(0));
    let page_size = i64::from(page_size.unwrap_or(50));
    let limit = page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = page * limit;

    let (files, total) = {
        let db = lock_db(&state.db)?;
        let files = FileRepo::list(db.conn(), category.as_deref(), offset, limit)
            .map_err(|e| format!("DB-U-001:数据读取失败，请重启应用 ({e})"))?;
        let total = FileRepo::count(db.conn(), category.as_deref())
            .map_err(|e| format!("DB-U-001:数据读取失败，请重启应用 ({e})"))?;
        drop(db);
        (files, total)
    };

    let file_infos: Vec<FileInfo> = files.into_iter().map(FileInfo::from).collect();
    Ok(FileListResponse {
        files: file_infos,
        total,
        page,
        page_size: limit,
    })
}

/// 全文搜索文件（基于 FTS5）。
///
/// # Errors
///
/// 数据库锁中毒或搜索失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn search_files(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let search_limit = i64::from(limit.unwrap_or(50)).clamp(1, MAX_PAGE_SIZE);

    let results = {
        let db = lock_db(&state.db)?;
        FileSearch::search(db.conn(), &query, search_limit)
            .map_err(|e| format!("DB-U-001:搜索失败 ({e})"))?
    };

    Ok(results)
}

/// 按文件名模式搜索文件。
///
/// # Errors
///
/// 数据库锁中毒或搜索失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn search_by_filename(
    state: State<'_, AppState>,
    pattern: String,
    limit: Option<u32>,
) -> Result<Vec<FileInfo>, String> {
    let search_limit = i64::from(limit.unwrap_or(50)).clamp(1, MAX_PAGE_SIZE);

    let files = {
        let db = lock_db(&state.db)?;
        FileSearch::search_by_filename(db.conn(), &pattern, search_limit)
            .map_err(|e| format!("DB-U-001:搜索失败 ({e})"))?
    };

    let file_infos: Vec<FileInfo> = files.into_iter().map(FileInfo::from).collect();
    Ok(file_infos)
}

/// 获取文件库统计信息（总数、已分类、重复组、总大小）。
///
/// # Errors
///
/// 数据库锁中毒或统计查询失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn get_file_stats(state: State<'_, AppState>) -> Result<FileStats, String> {
    let file_stats = {
        let db = lock_db(&state.db)?;

        let total: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

        let categorized: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND category IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

        let duplicates: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) - COUNT(DISTINCT content_hash) FROM files
             WHERE is_deleted = 0 AND content_hash IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;

        let total_size: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(file_size), 0) FROM files WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("DB-U-001:统计失败 ({e})"))?;
        drop(db);

        FileStats {
            total_files: total,
            categorized_files: categorized,
            uncategorized_files: total - categorized,
            duplicate_groups: duplicates,
            total_size_bytes: total_size,
        }
    };

    Ok(file_stats)
}

/// 更新指定文件的分类标签。
///
/// # Errors
///
/// 文件不存在时返回 `FILE-E-001`；更新失败返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn update_file_category(
    state: State<'_, AppState>,
    id: String,
    category: String,
) -> Result<(), String> {
    {
        let db = lock_db(&state.db)?;
        FileRepo::update_category(db.conn(), &id, &category).map_err(|e| match e {
            AppError::Database(rusqlite::Error::QueryReturnedNoRows) => {
                "FILE-E-001:文件不存在".to_string()
            }
            _ => format!("DB-U-001:更新失败 ({e})"),
        })?;
    }
    Ok(())
}

/// 加载数据库互斥锁，锁中毒时记录日志并返回用户可读错误。
fn lock_db(
    db: &Mutex<crate::db::Database>,
) -> Result<std::sync::MutexGuard<'_, crate::db::Database>, String> {
    db.lock().map_err(|e| {
        log::error!("DB lock poisoned: {e}");
        "DB-U-001:数据读取失败，请重启应用".to_string()
    })
}

/// 分页文件列表响应。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileListResponse {
    /// 当前页文件。
    pub files: Vec<FileInfo>,
    /// 满足条件的总条数。
    pub total: i64,
    /// 当前页码（从 0 开始）。
    pub page: i64,
    /// 每页条数（实际生效值）。
    pub page_size: i64,
}

/// 文件库统计。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileStats {
    /// 文件总数。
    pub total_files: i64,
    /// 已分类文件数。
    pub categorized_files: i64,
    /// 未分类文件数。
    pub uncategorized_files: i64,
    /// 疑似重复文件组数。
    pub duplicate_groups: i64,
    /// 文件总大小（字节）。
    pub total_size_bytes: i64,
}

// ----------------------------------------------------------------------
// T3.6 操作历史查询
// ----------------------------------------------------------------------

const HISTORY_DEFAULT_PAGE: i64 = 1;
const HISTORY_DEFAULT_PAGE_SIZE: i64 = 20;
const HISTORY_MAX_PAGE_SIZE: i64 = 100;

/// 分页获取操作历史（API §2-2d）。
///
/// `page` 从 1 开始（与 `list_files` 的从 0 开始不同，对齐 API 规格书）。
/// `can_undo` 三层条件：
///   1. DB 层（`list_history`）：`status == "done" && !has_delete`
///   2. IPC 层（本命令）：再注入 `is_within_window(created_at)`（24h 窗口）
///   3. 最终值 = 两条件 AND
///
/// # Errors
///
/// 数据库锁中毒或查询失败返回 `DB-U-002`；时间解析失败返回 `DB-U-003`。
#[tauri::command(async)]
#[specta::specta]
pub fn get_operation_history(
    state: State<'_, AppState>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<OperationHistoryResponse, String> {
    let page = i64::from(page.unwrap_or(1)).max(HISTORY_DEFAULT_PAGE);
    let page_size =
        i64::from(page_size.unwrap_or(20)).clamp(HISTORY_DEFAULT_PAGE_SIZE, HISTORY_MAX_PAGE_SIZE);

    let mut summaries = {
        let db = lock_db(&state.db)?;
        OperationRepo::list_history(db.conn(), page, page_size)
            .map_err(|e| format!("DB-U-002:操作历史读取失败 ({e})"))?
    };

    // 注入撤销窗口判断（DB 层只算 status + has_delete，窗口由这里算）
    for s in &mut summaries {
        let within = undo_window::is_within_window(&s.created_at)
            .map_err(|e| format!("DB-U-003:时间解析失败 ({e})"))?;
        s.can_undo = s.can_undo && within;
    }

    Ok(OperationHistoryResponse {
        batches: summaries,
        page,
        page_size,
    })
}

/// 查询单个批次的详细日志（API §2-2d 的 `batch_id` 参数分支）。
///
/// # Errors
///
/// 数据库锁中毒或查询失败返回 `DB-U-002`；批次不存在返回 `DB-U-004`。
#[tauri::command(async)]
#[specta::specta]
pub fn get_batch_detail(
    state: State<'_, AppState>,
    batch_id: String,
) -> Result<BatchDetailResponse, String> {
    let logs = {
        let db = lock_db(&state.db)?;
        OperationRepo::list_by_batch(db.conn(), &batch_id)
            .map_err(|e| format!("DB-U-002:批次详情读取失败 ({e})"))?
    };

    if logs.is_empty() {
        return Err("DB-U-004:批次不存在".to_string());
    }

    Ok(BatchDetailResponse { batch_id, logs })
}

/// 操作历史响应（API §2-2d 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationHistoryResponse {
    /// 批次摘要列表。
    pub batches: Vec<OperationBatchSummary>,
    /// 当前页码（从 1 开始）。
    pub page: i64,
    /// 每页条数（实际生效值）。
    pub page_size: i64,
}

/// 批次详情响应（API §2-2d `batch_id` 分支返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchDetailResponse {
    /// 批次 ID。
    pub batch_id: String,
    /// 批次内所有日志行（按 `created_at` 升序）。
    pub logs: Vec<OperationLog>,
}
