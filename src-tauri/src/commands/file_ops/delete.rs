//! 删除：移入系统回收站（安全网）+ 软删除 + 审计留痕，复用与分类删除同一执行器。

use crate::db::models::{FileRecord, OperationLog};
use crate::db::{FileRepo, OperationRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::PlanStatus;
use crate::services::operation_executor;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// 同层子模块：拆分前同文件内直接可见，拆后需显式引入
use super::preview::{OperationType, PlanItem};

/// 单个文件删除结果（移入系统回收站）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DeleteFilesResult {
    /// 文件 id。
    pub file_id: String,
    /// 是否成功移入系统回收站。
    pub success: bool,
    /// 失败原因（成功时为 `None`）。
    pub error: Option<String>,
}

/// 删除失败结果的简构（避免重复三元组）。
fn delete_files_failure(file_id: String, reason: &str) -> DeleteFilesResult {
    DeleteFilesResult {
        file_id,
        success: false,
        error: Some(reason.to_string()),
    }
}

/// 删除文件：把选中的文件移入系统回收站（安全网，非物理删除）。
///
/// 逐个执行（单个失败不阻断其余文件）：
///   1. 路径安全校验 + 磁盘文件存在性检查
///   2. 移入系统回收站（`operation_executor` 的 `Delete` 语义）
///   3. 成功后 `files` 软删除（`is_deleted=1`）并写入 `operations_log` 审计行
///
/// 与分类预览的 `Delete` 计划共用同一执行器，保证删除语义唯一。删除批次
/// 不参与应用内撤销（文件可从系统回收站手动恢复）。
///
/// # Errors
///
/// 入参全为不可删除（空/全部不存在）时返回成功空列表；单文件失败以
/// `DeleteFilesResult.success=false` 返回，不中断整批。
#[tauri::command(async)]
#[specta::specta]
pub fn delete_files(
    file_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DeleteFilesResult>, String> {
    delete_files_inner(&file_ids, &state).map_err(|e| e.to_string())
}

/// 删除纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
pub(super) fn delete_files_inner(
    file_ids: &[String],
    state: &AppState,
) -> AppResult<Vec<DeleteFilesResult>> {
    // 去重（保序）：同一文件重复提交只删一次
    let mut seen = HashSet::with_capacity(file_ids.len());
    let mut ids: Vec<String> = Vec::with_capacity(file_ids.len());
    for file_id in file_ids {
        if seen.insert(file_id) {
            ids.push(file_id.clone());
        }
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let records = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::get_by_ids(guard.conn(), &ids)?
    };
    let record_map: HashMap<String, FileRecord> =
        records.into_iter().map(|r| (r.id.clone(), r)).collect();

    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut results = Vec::with_capacity(ids.len());
    let mut logs_to_insert: Vec<OperationLog> = Vec::new();

    for file_id in ids {
        let Some(record) = record_map.get(&file_id) else {
            results.push(delete_files_failure(file_id, "文件不存在或已删除"));
            continue;
        };
        if record.is_deleted {
            results.push(delete_files_failure(file_id, "文件已删除"));
            continue;
        }
        let source = match security::validate(&record.path) {
            Ok(path) => path,
            Err(e) => {
                results.push(delete_files_failure(file_id, &e.to_string()));
                continue;
            }
        };
        if !source.is_file() {
            results.push(delete_files_failure(
                file_id,
                "文件不存在（可能已被外部移除）",
            ));
            continue;
        }

        let item = PlanItem {
            file_id: file_id.clone(),
            file_name: record.file_name.clone(),
            original_path: record.path.clone(),
            new_path: None,
            operation: OperationType::Delete,
            status: PlanStatus::Ok,
            conflict_type: None,
        };
        let (success, error, current_hash) =
            operation_executor::execute_plan_item(&item, record.content_hash.clone());

        if success {
            // 软删除：磁盘已进回收站，DB 标记 is_deleted=1（失败仅告警）
            if let Ok(guard) = state.db.lock() {
                if let Err(e) = FileRepo::soft_delete(guard.conn(), &file_id) {
                    log::warn!("delete_files soft_delete 失败（不影响删除结果）: {e}");
                }
            }
        }

        logs_to_insert.push(OperationLog {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: batch_id.clone(),
            operation_type: "delete".to_string(),
            source_path: record.path.clone(),
            target_path: String::new(),
            status: if success { "done" } else { "failed" }.into(),
            prev_hash: record.content_hash.clone().unwrap_or_default(),
            current_hash: current_hash.unwrap_or_default(),
            chain_hash: String::new(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });
        results.push(DeleteFilesResult {
            file_id,
            success,
            error,
        });
    }

    // 批量写审计日志（链式哈希由 OperationRepo::insert_batch 计算）
    if !logs_to_insert.is_empty() {
        if let Ok(guard) = state.db.lock() {
            if let Err(e) = OperationRepo::insert_batch(guard.conn(), &logs_to_insert) {
                log::warn!("delete_files 写 operations_log 失败（不影响删除结果）: {e}");
            }
        }
    }

    Ok(results)
}
