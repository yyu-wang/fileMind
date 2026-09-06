//! 操作执行服务：按 `PlanItem` 执行单个文件操作，返回执行结果。
//!
//! 设计：
//! - 纯函数式，接收 `&PlanItem` + `prev_hash`，返回 `(success, error, current_hash)`
//! - 不直接访问 `SQLite`（保持 services 单向依赖，调用方负责 DB 更新）
//! - 失败时返回 `success=false` + `error=Some(msg)`，不 `panic`
//! - `Move`/`Rename`/`Copy` 操作前自动 `create_dir_all` 父目录，支持跨目录移动

use crate::commands::file_ops::{OperationType, PlanItem};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::PlanStatus;

/// 执行单个 `plan` item：按 `operation_type` 调 `fs::rename` / `fs::copy` / `fs::remove_file`。
///
/// 入参：
///   - `item`：预览阶段生成的 plan（含 `original_path` / `new_path` / `operation` / `status`）
///   - `prev_hash`：执行前的 `content_hash`（调用方从 `SQLite` 查出，用于写日志）
///
/// 返回 `(success, error, current_hash)`：
///   - 成功：`success=true, error=None, current_hash=Some(prev_hash)`（`Delete` 时为 `None`）
///   - 失败：`success=false, error=Some(msg), current_hash=None`
///
/// 防御性检查：`item.status != Ok` 时直接返回失败（调用方应已过滤，这里兜底）。
#[must_use]
pub fn execute_plan_item(
    item: &PlanItem,
    prev_hash: Option<String>,
) -> (bool, Option<String>, Option<String>) {
    if item.status != PlanStatus::Ok {
        return (
            false,
            Some(format!("plan 状态非 Ok: {:?}", item.status)),
            None,
        );
    }

    let result: AppResult<Option<String>> = run_operation(item, prev_hash);

    match result {
        Ok(current_hash) => (true, None, current_hash),
        Err(e) => (false, Some(e.to_string()), None),
    }
}

/// 实际执行文件系统操作。
///
/// 返回执行后的 `current_hash`：
///   - `Move`/`Rename`：源文件已移动，`current_hash = prev_hash`（内容未变）
///   - `Copy`：源文件保留，新副本内容一致，`current_hash = prev_hash`
///   - `Delete`：源文件已删除，`current_hash = None`
fn run_operation(item: &PlanItem, prev_hash: Option<String>) -> AppResult<Option<String>> {
    let source = security::validate(&item.original_path)?;

    match item.operation {
        OperationType::Move | OperationType::Rename => {
            let target = item
                .new_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidInput("Move/Rename 缺少 new_path".into()))?;
            let target = security::validate_write_target(target)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&source, &target)?;
            Ok(prev_hash)
        }
        OperationType::Copy => {
            let target = item
                .new_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidInput("Copy 缺少 new_path".into()))?;
            let target = security::validate_write_target(target)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &target)?;
            Ok(prev_hash)
        }
        OperationType::Delete => {
            // 安全网：删除改为移入系统回收站（可恢复），替代物理 remove_file
            crate::services::trash::move_to_trash(&source)?;
            Ok(None)
        }
    }
}
