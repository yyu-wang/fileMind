//! 扫描目录管理：已扫描目录列表与移除（含移除后的索引清理）。

use crate::db::{FileRepo, ScannedDirectoryRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::AppState;
use serde::{Deserialize, Serialize};

// 同层子模块：拆分前同文件内直接可见，拆后需显式引入
use super::index_sync::spawn_index_delete_by_file_ids;

/// 移除目录响应体。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoveDirectoryResponse {
    /// 从索引中移除的文件数。
    #[specta(type = specta_typescript::Number)]
    pub removed_files: i64,
}

/// 列出所有已扫描目录（含每个目录的文件数）。
///
/// # Errors
///
/// DB 锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn list_scanned_directories(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::db::ScannedDirectory>, String> {
    let guard = state.db.lock().map_err(|e| format!("DB 锁中毒: {e}"))?;
    ScannedDirectoryRepo::list_with_file_count(guard.conn()).map_err(|e| e.to_string())
}

/// 移除目录：软删该目录下所有文件 + 清理向量索引 + 删除目录记录。
///
/// 不删除磁盘文件，仅从 `FileMind` 索引中移除。重新扫描该目录即可恢复。
///
/// 流程：
///   1. 路径安全校验
///   2. 获取该目录下所有未软删除文件的 ID
///   3. best-effort 调 sidecar 清理对应向量（失败仅告警）
///   4. 软删该目录下所有文件，并同步清空其索引状态标记（防重扫后误跳过）
///   5. 删除目录记录
///
/// # Errors
///
/// 路径未通过安全校验或 DB 操作失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn remove_directory(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<RemoveDirectoryResponse, String> {
    remove_directory_inner(&path, &state).map_err(|e| e.to_string())
}

/// `remove_directory` 纯逻辑入口（便于测试）。
fn remove_directory_inner(path: &str, state: &AppState) -> AppResult<RemoveDirectoryResponse> {
    // 1. 路径安全校验
    let _safe_path = security::validate(path)?;

    // 2. 获取该目录下所有未软删除文件的 ID（用于清理向量）
    let file_ids = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::get_ids_by_path_prefix(guard.conn(), path)?
    };

    // 3. best-effort 清理向量索引（sidecar 未就绪或失败仅告警）
    if !file_ids.is_empty() {
        spawn_index_delete_by_file_ids(state, file_ids.clone());
    }

    // 4. 软删该目录下所有文件 + 同步清空索引状态标记。
    //    marker 必须清空：否则向量已删但标记仍在，之后重新扫描同一目录时，
    //    增量索引会把复活文件误判为「已建」而跳过（见 FileRepo::clear_embedding_marker）。
    let removed_files = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let removed = FileRepo::soft_delete_by_path_prefix(guard.conn(), path)?;
        if let Err(e) = FileRepo::clear_embedding_marker(guard.conn(), &file_ids) {
            log::warn!("移除目录：清空索引状态标记失败（不影响移除结果）: {e}");
        }
        drop(guard);
        removed
    };

    // 5. 删除目录记录
    {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        ScannedDirectoryRepo::delete(guard.conn(), path)?;
    }

    Ok(RemoveDirectoryResponse { removed_files })
}
