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
    let ids = dedupe_ids(file_ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let record_map = load_record_map(state, &ids)?;
    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut results = Vec::with_capacity(ids.len());
    let mut logs_to_insert: Vec<OperationLog> = Vec::new();

    for file_id in ids {
        let record = record_map.get(&file_id);
        let (result, log) = delete_one(state, file_id, record, &batch_id);
        if let Some(log) = log {
            logs_to_insert.push(log);
        }
        results.push(result);
    }

    insert_logs(state, &logs_to_insert);
    Ok(results)
}

/// 去重（保序）：同一文件重复提交只删一次。
fn dedupe_ids(file_ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(file_ids.len());
    let mut ids: Vec<String> = Vec::with_capacity(file_ids.len());
    for file_id in file_ids {
        if seen.insert(file_id) {
            ids.push(file_id.clone());
        }
    }
    ids
}

/// 按 id 一次性取全文件记录，避免循环内反复加锁。
fn load_record_map(state: &AppState, ids: &[String]) -> AppResult<HashMap<String, FileRecord>> {
    let guard = state
        .db
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
    let records = FileRepo::get_by_ids(guard.conn(), ids)?;
    drop(guard);
    Ok(records.into_iter().map(|r| (r.id.clone(), r)).collect())
}

/// 删除前置校验：不存在 / 已删除 / 路径不安全 / 磁盘上缺失 → 返回失败结果。
fn precheck_deletable<'a>(
    file_id: &str,
    record: Option<&'a FileRecord>,
) -> Result<&'a FileRecord, DeleteFilesResult> {
    let Some(record) = record else {
        return Err(delete_files_failure(
            file_id.to_string(),
            "文件不存在或已删除",
        ));
    };
    if record.is_deleted {
        return Err(delete_files_failure(file_id.to_string(), "文件已删除"));
    }
    match security::validate(&record.path) {
        Ok(path) if path.is_file() => Ok(record),
        Ok(_) => Err(delete_files_failure(
            file_id.to_string(),
            "文件不存在（可能已被外部移除）",
        )),
        Err(e) => Err(delete_files_failure(file_id.to_string(), &e.to_string())),
    }
}

/// 构造该文件的审计日志行（链式哈希由 `OperationRepo::insert_batch` 计算）。
fn build_delete_log(
    batch_id: &str,
    record: &FileRecord,
    success: bool,
    current_hash: Option<String>,
) -> OperationLog {
    OperationLog {
        id: uuid::Uuid::new_v4().to_string(),
        batch_id: batch_id.to_string(),
        operation_type: "delete".to_string(),
        source_path: record.path.clone(),
        target_path: String::new(),
        status: if success { "done" } else { "failed" }.into(),
        prev_hash: record.content_hash.clone().unwrap_or_default(),
        current_hash: current_hash.unwrap_or_default(),
        chain_hash: String::new(),
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

/// 删除单个文件：移入系统回收站 → 成功后软删除。返回（结果，审计日志行）。
///
/// 前置校验不通过时不产生审计行——与拆分前一致：未进入执行器的项不记日志。
fn delete_one(
    state: &AppState,
    file_id: String,
    record: Option<&FileRecord>,
    batch_id: &str,
) -> (DeleteFilesResult, Option<OperationLog>) {
    let record = match precheck_deletable(&file_id, record) {
        Ok(record) => record,
        Err(failure) => return (failure, None),
    };

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

    let log = build_delete_log(batch_id, record, success, current_hash);
    (
        DeleteFilesResult {
            file_id,
            success,
            error,
        },
        Some(log),
    )
}

/// 批量写审计日志（失败仅告警，不影响删除结果）。
fn insert_logs(state: &AppState, logs: &[OperationLog]) {
    if logs.is_empty() {
        return;
    }
    if let Ok(guard) = state.db.lock() {
        if let Err(e) = OperationRepo::insert_batch(guard.conn(), logs) {
            log::warn!("delete_files 写 operations_log 失败（不影响删除结果）: {e}");
        }
    }
}
