//! 撤销：按批次反向执行操作链（移动回滚 / 复制清理）；`delete` 批次不支持应用内撤销。

use crate::db::models::OperationLog;
use crate::db::{FileRepo, OperationRepo};
use crate::error::{AppError, AppResult};
use crate::services::undo_executor;

// 同层子模块：拆分前同文件内直接可见，拆后需显式引入
use super::index_sync::spawn_index_path_sync;
use crate::AppState;
use serde::{Deserialize, Serialize};

/// 按批次 ID 撤销已执行的批量操作（API §s2-2c）。
///
/// 流程（反向操作链）：
///   1. `OperationRepo::list_by_batch` 拉取该批次日志（ASC 序）
///   2. 校验：批次不存在 / 已撤销 / 含 `delete`（当前不支持）→ 报错
///   3. 反向遍历（DESC），逐条对 `status=done` 的行执行反向文件操作：
///      - `move`/`rename`：`rename(target → source)` + 回写 `files.path`
///      - `copy`：删除 `target_path` 处的副本
///   4. 每项成功后 `update_status(undone)`；单项失败计入 `failed_count`
///
/// # Errors
///
/// 批次不存在、已撤销或含 `delete` 时返回错误；单项撤销失败记录在 `failed_count` 中。
#[tauri::command(async)]
#[specta::specta]
pub fn undo_batch(
    batch_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<UndoResponse, String> {
    undo_batch_inner(&batch_id, &state).map_err(|e| e.to_string())
}

/// 撤销逻辑纯函数入口（便于单元测试，不依赖 `tauri::State`）。
pub(super) fn undo_batch_inner(batch_id: &str, state: &AppState) -> AppResult<UndoResponse> {
    let logs = load_undoable_logs(state, batch_id)?;

    let task_id = uuid::Uuid::new_v4().to_string();
    let mut undone_count = 0u32;
    let mut failed_count = 0u32;
    // 路径被移回的文件（file_id, 恢复的原路径），撤销后同步到向量索引
    let mut restored_paths: Vec<(String, String)> = Vec::new();

    // 反向遍历：后执行的先撤销（同类操作互相独立，反序仅为语义正确性）
    for log in logs.iter().rev() {
        // 只撤销执行成功的项；failed/pending 无文件副作用可回滚，跳过
        if log.status != "done" {
            continue;
        }
        if undo_one(state, log, &mut restored_paths) {
            undone_count += 1;
        } else {
            failed_count += 1;
        }
    }

    // 尽力而为：把移回原路径的文件同步到向量索引（失败仅告警）
    spawn_index_path_sync(state, restored_paths);

    Ok(UndoResponse {
        success: failed_count == 0,
        undone_count,
        failed_count,
        task_id,
    })
}

/// 拉取批次日志并做三项可撤销性校验（批次存在 / 未撤销过 / 不含 delete）。
fn load_undoable_logs(state: &AppState, batch_id: &str) -> AppResult<Vec<OperationLog>> {
    let logs = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        OperationRepo::list_by_batch(guard.conn(), batch_id)?
    };

    if logs.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "批次不存在或无可撤销项: {batch_id}"
        )));
    }

    // 幂等校验：批次内已有 undone → 拒绝重复撤销
    if logs.iter().any(|l| l.status == "undone") {
        return Err(AppError::Forbidden(format!(
            "批次已撤销，不能重复撤销: {batch_id}"
        )));
    }

    // 删除已移入系统回收站：应用内无回收站还原路径，整批拒绝撤销
    // （用户可在系统回收站手动恢复）
    if logs.iter().any(|l| l.operation_type == "delete") {
        return Err(AppError::Forbidden("删除批次暂不支持撤销".into()));
    }

    Ok(logs)
}

/// 撤销单项：物理反向操作 → 成功后同步 DB。返回是否成功（失败仅告警，不中断整批）。
fn undo_one(
    state: &AppState,
    log: &OperationLog,
    restored_paths: &mut Vec<(String, String)>,
) -> bool {
    match undo_executor::execute_undo_item(&log.operation_type, &log.source_path, &log.target_path)
    {
        Ok(()) => {
            sync_undone_to_db(state, log, restored_paths);
            true
        }
        Err(e) => {
            log::warn!("undo_batch 撤销项失败 (id={}): {e}", log.id);
            false
        }
    }
}

/// 撤销后的 DB 收尾：`move`/`rename` 回写 `files.path` 并记入 `restored_paths`，
/// 随后把日志标记 `undone`。`copy` 撤销只删副本，`files` 表源记录不变。
fn sync_undone_to_db(
    state: &AppState,
    log: &OperationLog,
    restored_paths: &mut Vec<(String, String)>,
) {
    let Ok(guard) = state.db.lock() else {
        return;
    };
    let conn = guard.conn();
    if matches!(log.operation_type.as_str(), "move" | "rename") {
        if let Ok(Some(rec)) = FileRepo::get_by_path(conn, &log.target_path) {
            if let Err(e) = FileRepo::update_path(conn, &rec.id, &log.source_path) {
                log::warn!("undo_batch 回写 files.path 失败: {e}");
            } else {
                restored_paths.push((rec.id.clone(), log.source_path.clone()));
            }
        }
    }
    if let Err(e) = OperationRepo::update_status(conn, &log.id, "undone") {
        log::warn!("undo_batch 标记 undone 失败: {e}");
    }
}

/// 撤销响应体（API §s2-2c 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UndoResponse {
    /// 是否全部撤销成功（无失败项）。
    pub success: bool,
    /// 成功撤销条数。
    pub undone_count: u32,
    /// 撤销失败条数（原路径被占用等）。
    pub failed_count: u32,
    /// 本次撤销任务 ID（uuid，用于关联日志/审计）。
    pub task_id: String,
}
