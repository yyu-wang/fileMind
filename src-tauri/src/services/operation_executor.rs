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
        Err(e) => {
            log::warn!(
                "execute_plan_item 失败: op={:?} source={} target={:?} error={e}",
                item.operation,
                item.original_path,
                item.new_path
            );
            (false, Some(e.to_string()), None)
        }
    }
}

/// 实际执行文件系统操作。
///
/// 返回执行后的 `current_hash`：
///   - `Move`/`Rename`：源文件已移动，`current_hash = prev_hash`（内容未变）
///   - `Copy`：源文件保留，新副本内容一致，`current_hash = prev_hash`
///   - `Delete`：源文件已删除，`current_hash = None`
///
/// macOS 14.4+ 防御：`com.apple.provenance` xattr 会阻止 rename/copy，
/// 遇到 EPERM 时自动清除该属性并重试一次。
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
            rename_with_provenance_retry(&source, &target)?;
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
            copy_with_provenance_retry(&source, &target)?;
            Ok(prev_hash)
        }
        OperationType::Delete => {
            // 安全网：删除改为移入系统回收站（可恢复），替代物理 remove_file
            crate::services::trash::move_to_trash(&source)?;
            Ok(None)
        }
    }
}

/// 带 provenance 清除重试的 rename：EPERM 时清除源文件的
/// `com.apple.provenance` xattr（macOS 14.4+ 溯源标记）后重试一次。
fn rename_with_provenance_retry(
    source: &std::path::Path,
    target: &std::path::Path,
) -> AppResult<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            if try_clear_provenance(source) {
                log::info!(
                    "rename 因 provenance EPERM，已清除属性后重试: {}",
                    source.display()
                );
                std::fs::rename(source, target).map_err(AppError::Io)
            } else {
                Err(AppError::Io(e))
            }
        }
        Err(e) => Err(AppError::Io(e)),
    }
}

/// 带 provenance 清除重试的 copy：EPERM 时清除源文件的
/// `com.apple.provenance` xattr 后重试一次。
fn copy_with_provenance_retry(source: &std::path::Path, target: &std::path::Path) -> AppResult<()> {
    match std::fs::copy(source, target) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            if try_clear_provenance(source) {
                log::info!(
                    "copy 因 provenance EPERM，已清除属性后重试: {}",
                    source.display()
                );
                std::fs::copy(source, target)
                    .map(|_| ())
                    .map_err(AppError::Io)
            } else {
                Err(AppError::Io(e))
            }
        }
        Err(e) => Err(AppError::Io(e)),
    }
}

/// 尝试清除 macOS 的 `com.apple.provenance` xattr。
///
/// 注意：macOS 的 `xattr` 不支持 `-f`（那是 Linux 选项）。属性不存在时
/// `xattr -d` 会返回非 0 退出码，但属性存在时清除成功返回 0。
/// 返回 `true` 表示属性已清除，`false` 表示清除失败（受 SIP 保护时连 root 也清不掉）。
/// 非 macOS 平台恒返回 `false`（no-op）。
#[cfg(target_os = "macos")]
fn try_clear_provenance(path: &std::path::Path) -> bool {
    std::process::Command::new("/usr/bin/xattr")
        .args(["-d", "com.apple.provenance"])
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(target_os = "macos"))]
const fn try_clear_provenance(_path: &std::path::Path) -> bool {
    false
}
