//! 文件查询命令：分页列表、全文搜索、文件名搜索与统计。
//!
//! 所有命令访问 SQLite（阻塞 IO），通过 `#[tauri::command(async)]`
//! 声明为线程池执行；数据库锁的作用域收窄到查询完成即释放。
//!
//! 拆分（原单文件 375 行，逼近 Rust 模块 500 行强制阈值）：
//!   - `commands/file_query_types.rs`  IPC 响应类型（specta 契约）
//!   - `commands/file_query_stats.rs`  统计的 SQL 聚合
//! 八个 `#[tauri::command]` 留在本模块，注册路径不变。

use std::sync::Mutex;

use tauri::State;

use crate::commands::file_query_stats::query_stats;
use crate::commands::file_query_types::{
    BatchDetailResponse, FileListResponse, FileStats, OperationHistoryResponse,
};
use crate::db::{FileRepo, FileSearch, OperationRepo, SearchResult};
use crate::error::{AppError, AppResult};
use crate::services::undo_window;
use crate::{AppState, FileInfo};

const MAX_PAGE_SIZE: i64 = 500;
/// 全量列表上限：支撑前端虚拟滚动一次取回全部文件（10K 级），远超单库现实规模。
const LIST_ALL_LIMIT: i64 = 1_000_000;

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

/// 全量获取文件列表（可按分类过滤），供前端虚拟滚动一次取回全部数据。
///
/// 与 `list_files` 的分页语义不同：`list_files` 面向增量翻页（上限 500/页），
/// 本命令面向虚拟滚动表（需完整数组做客户端筛选/排序），按 `updated_at DESC` 返回全部。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn list_all_files(
    state: State<'_, AppState>,
    category: Option<String>,
) -> Result<Vec<FileInfo>, String> {
    let files = {
        let db = lock_db(&state.db)?;
        list_all_from_conn(db.conn(), category.as_deref())
            .map_err(|e| format!("DB-U-001:数据读取失败，请重启应用 ({e})"))?
    };

    Ok(files)
}

/// 全量列表的纯逻辑入口（便于单元测试，直接操作连接）。
fn list_all_from_conn(
    conn: &rusqlite::Connection,
    category: Option<&str>,
) -> AppResult<Vec<FileInfo>> {
    let files = FileRepo::list(conn, category, 0, LIST_ALL_LIMIT)?;
    Ok(files.into_iter().map(FileInfo::from).collect())
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
/// 聚合 SQL 见 `file_query_stats::query_stats`。
///
/// # Errors
///
/// 数据库锁中毒或统计查询失败时返回 `DB-U-001`。
#[tauri::command(async)]
#[specta::specta]
pub fn get_file_stats(state: State<'_, AppState>) -> Result<FileStats, String> {
    let db = lock_db(&state.db)?;
    query_stats(db.conn()).map_err(|e| format!("DB-U-001:统计失败 ({e})"))
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]
#[path = "file_query_tests.rs"]
mod tests;
