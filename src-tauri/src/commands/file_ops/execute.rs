//! 批量执行：按 plan 落盘并同步 `SQLite`（移动/重命名/复制/软删除）+ 审计日志。

use crate::db::models::OperationLog;
use crate::db::{FileRepo, OperationRepo};
use crate::error::{AppError, AppResult};
use crate::services::conflict_resolver::{self, ConflictStrategy, PlanStatus};
use crate::services::operation_executor;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// 同层子模块：拆分前同文件内直接可见，拆后需显式引入
use super::index_sync::spawn_index_path_sync;
use super::preview::{OperationType, PlanItem};

/// 批量执行文件操作（API §s2-2b）。
///
/// 流程：
///   1. 接收 `ExecuteRequest { batch_id, plan, exclude_file_ids }`
///   2. 预取所有 `file_id → prev_hash`（一次 `FileRepo::get_by_ids`，避免循环 lock）
///   3. 逐项调 `operation_executor::execute_plan_item` 执行
///      - 跳过 `exclude_file_ids` 中的项（计入 `summary.skipped`）
///      - 跳过非 `Ok` 状态的 plan（Conflict/Error，计入 `summary.skipped`）
///   4. 成功后更新 `SQLite` `files` 表：
///      - `Move`/`Rename`：`update_path`（路径变了，文件还在）
///      - `Copy`：源记录不变，新副本留给下次 `scan_directory` 入库
///      - `Delete`：`soft_delete`（标记 `is_deleted=1`，便于 T3.4 undo 恢复）
///   5. 循环结束后一次 `OperationRepo::insert_batch` 写日志（失败只记 warn）
///
/// # Errors
///
/// 仅在严重错误（DB 锁中毒）时返回；单项失败记录在 `results` 中。
#[tauri::command(async)]
#[specta::specta]
pub fn execute_operations(
    request: ExecuteRequest,
    state: tauri::State<'_, AppState>,
) -> Result<ExecuteResponse, String> {
    let response = execute_operations_inner(request, &state).map_err(|e| e.to_string())?;
    Ok(response)
}

/// 执行逻辑纯函数入口（便于单元测试，不依赖 `tauri::State`）。
#[allow(clippy::too_many_lines)]
pub(super) fn execute_operations_inner(
    request: ExecuteRequest,
    state: &AppState,
) -> AppResult<ExecuteResponse> {
    let exclude_set: HashSet<String> = request.exclude_file_ids.iter().cloned().collect();

    let mut results = Vec::with_capacity(request.plan.len());
    let mut summary = ExecuteSummary {
        total: u32::try_from(request.plan.len()).unwrap_or(u32::MAX),
        ..Default::default()
    };

    // 预取所有 file_id → prev_hash（避免循环里反复 lock）
    let file_ids: Vec<String> = request.plan.iter().map(|p| p.file_id.clone()).collect();
    let prev_hash_map: HashMap<String, Option<String>> = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let records = FileRepo::get_by_ids(guard.conn(), &file_ids)?;
        drop(guard);
        records
            .into_iter()
            .map(|r| (r.id, r.content_hash))
            .collect()
    };

    let mut logs_to_insert: Vec<OperationLog> = Vec::new();
    // 路径被移动/重命名的文件（file_id, 新路径），执行后同步到向量索引
    let mut moved_paths: Vec<(String, String)> = Vec::new();

    for item in &request.plan {
        // 跳过用户排除的文件
        if exclude_set.contains(&item.file_id) {
            summary.skipped += 1;
            continue;
        }

        // 「确认执行全部」（resolve_conflicts=true）：冲突项按 Rename 策略重算目标路径
        // （stem_1.ext 递增到不冲突），纳入执行；否则冲突项与 Error 项一律跳过。
        let resolved = if item.status == PlanStatus::Conflict && request.resolve_conflicts {
            item.new_path.as_ref().and_then(|np| {
                Path::new(np).parent().map(|dir| {
                    let (path, status, _) = conflict_resolver::resolve(
                        &item.file_name,
                        Path::new(&item.original_path),
                        dir,
                        ConflictStrategy::Rename,
                    );
                    let mut r = item.clone();
                    r.new_path = path.map(|p| p.to_string_lossy().to_string());
                    r.status = status;
                    r
                })
            })
        } else {
            None
        };

        // 解析后仍非 Ok（非冲突项 / 未要求解析 / 重算异常）→ 跳过
        if item.status != PlanStatus::Ok
            && !matches!(&resolved, Some(r) if r.status == PlanStatus::Ok)
        {
            summary.skipped += 1;
            results.push(ExecuteResult {
                file_id: item.file_id.clone(),
                operation: item.operation.clone(),
                source_path: item.original_path.clone(),
                target_path: item.new_path.clone(),
                success: false,
                error: Some(format!("plan 状态非 Ok: {:?}", item.status)),
                prev_hash: None,
                current_hash: None,
            });
            continue;
        }

        // 实际执行目标：Rename 重算后的项（冲突项）或原项（Ok 项）
        let exec_item = resolved.as_ref().unwrap_or(item);

        let prev_hash = prev_hash_map.get(&exec_item.file_id).cloned().flatten();
        let (success, error, current_hash) =
            operation_executor::execute_plan_item(exec_item, prev_hash.clone());

        if success {
            summary.success += 1;
        } else {
            summary.failed += 1;
        }

        // 构造 operations_log 行（即使失败也写日志，便于审计）
        let log = OperationLog {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: request.batch_id.clone(),
            operation_type: format!("{:?}", exec_item.operation).to_lowercase(),
            source_path: exec_item.original_path.clone(),
            target_path: exec_item.new_path.clone().unwrap_or_default(),
            status: if success { "done" } else { "failed" }.into(),
            prev_hash: prev_hash.clone().unwrap_or_default(),
            current_hash: current_hash.clone().unwrap_or_default(),
            // 链式哈希由 OperationRepo::insert_batch 计算，这里占位空字符串
            chain_hash: String::new(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        logs_to_insert.push(log);

        // 成功后更新 files 表（path / updated_at / 软删除）
        if success {
            if let Ok(guard) = state.db.lock() {
                let conn = guard.conn();
                match exec_item.operation {
                    OperationType::Delete => {
                        if let Err(e) = FileRepo::soft_delete(conn, &exec_item.file_id) {
                            log::warn!(
                                "execute_operations soft_delete 失败（不影响执行结果）: {e}"
                            );
                        }
                    }
                    OperationType::Move | OperationType::Rename => {
                        if let Some(new_path) = &exec_item.new_path {
                            if let Err(e) =
                                FileRepo::update_path(conn, &exec_item.file_id, new_path)
                            {
                                log::warn!(
                                    "execute_operations update_path 失败（不影响执行结果）: {e}"
                                );
                            } else {
                                moved_paths.push((exec_item.file_id.clone(), new_path.clone()));
                            }
                        }
                    }
                    OperationType::Copy => {
                        // Copy 创建副本，源文件记录不变；新副本不入库（留给后续 scan_directory）
                    }
                }
            }
        }

        results.push(ExecuteResult {
            file_id: exec_item.file_id.clone(),
            operation: exec_item.operation.clone(),
            source_path: exec_item.original_path.clone(),
            target_path: exec_item.new_path.clone(),
            success,
            error,
            prev_hash,
            current_hash,
        });
    }

    // 批量写日志（一次事务）
    if !logs_to_insert.is_empty() {
        if let Ok(guard) = state.db.lock() {
            if let Err(e) = OperationRepo::insert_batch(guard.conn(), &logs_to_insert) {
                log::warn!("execute_operations 写 operations_log 失败（不影响执行结果）: {e}");
            }
        }
    }

    // 尽力而为：把移动后文件的最新路径同步到向量索引（失败仅告警）
    spawn_index_path_sync(state, moved_paths);

    Ok(ExecuteResponse {
        batch_id: request.batch_id,
        results,
        summary,
    })
}

/// 执行请求体（API §s2-2b）。
///
/// 入参 `batch_id` 来自 `preview_operations` 返回值，`plan` 透传该返回值。
/// `exclude_file_ids` 允许用户在预览后取消勾选某些文件，执行时跳过这些 ID。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteRequest {
    /// 来自预览的批次 ID（用于关联 plan 与 `operations_log` 日志）。
    pub batch_id: String,
    /// 透传 `preview_operations` 返回的 plan。
    pub plan: Vec<PlanItem>,
    /// 用户在预览后取消勾选的文件 ID 列表（执行时跳过这些项，计入 summary.skipped）。
    pub exclude_file_ids: Vec<String>,
    /// 是否解析冲突项：`true` 时对 `Conflict` 项按 Rename 策略重算目标路径一并执行
    /// （对应「确认执行全部」）；`false` 时冲突项跳过（对应「仅执行无冲突项」）。
    pub resolve_conflicts: bool,
}

/// 单项执行结果（API §s2-2b `results[]`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteResult {
    /// 文件 ID。
    pub file_id: String,
    /// 操作类型。
    pub operation: OperationType,
    /// 源路径。
    pub source_path: String,
    /// 目标路径（`Delete` 操作时为 `None`）。
    pub target_path: Option<String>,
    /// 是否执行成功。
    pub success: bool,
    /// 失败原因（成功时为 `None`）。
    pub error: Option<String>,
    /// 执行前内容哈希（从 `SQLite` 查出，用于写 `operations_log`）。
    pub prev_hash: Option<String>,
    /// 执行后内容哈希（`Delete` 时为 `None`；`Move`/`Copy` 等于 `prev_hash`）。
    pub current_hash: Option<String>,
}

/// 执行汇总（API §s2-2b `summary`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct ExecuteSummary {
    /// 总条目数（等于 `plan.len()`，含 skipped）。
    pub total: u32,
    /// 成功条数。
    pub success: u32,
    /// 失败条数。
    pub failed: u32,
    /// 跳过条数（`exclude_file_ids` 排除 + 非 `Ok` 状态 plan）。
    pub skipped: u32,
}

/// 执行响应体（API §s2-2b 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteResponse {
    /// 批次 ID（与请求的 `batch_id` 一致）。
    pub batch_id: String,
    /// 逐项结果。
    pub results: Vec<ExecuteResult>,
    /// 汇总统计。
    pub summary: ExecuteSummary,
}
