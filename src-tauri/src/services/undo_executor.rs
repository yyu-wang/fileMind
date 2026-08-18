//! 撤销执行服务：按 `operations_log` 行执行反向文件操作。
//!
//! 设计：
//! - 纯函数式，接收日志行的 `operation_type` / `source_path` / `target_path`，
//!   返回 `AppResult<()>`，不直接访问 `SQLite`
//! - `source_path` 为操作前路径，`target_path` 为操作后路径（`delete` 时为空）
//! - 反向语义：`move`/`rename` 把文件从 `target_path` 移回 `source_path`；
//!   `copy` 删除 `target_path` 处的副本（源文件不动）

use crate::error::{AppError, AppResult};
use crate::security;

/// 执行单条操作日志的反向操作（撤销）。
///
/// `operation_type` 取值与 `execute_operations` 写入 `operations_log` 的
/// 小写形式一致：`move` / `rename` / `copy` / `delete`。
///
/// # Errors
///
/// 路径校验失败、目标被占用或 IO 失败时返回错误（调用方按项计入 `failed`）。
/// `delete` 因 T3.3 为永久删除（物理文件无法恢复）返回 `Forbidden`。
pub fn execute_undo_item(
    operation_type: &str,
    source_path: &str,
    target_path: &str,
) -> AppResult<()> {
    match operation_type {
        "move" | "rename" => {
            // 当前所在位置（move 的目标）必须存在；原路径走写目标校验（可能被占用）
            let current = security::validate(target_path)?;
            let original = security::validate_write_target(source_path)?;
            if let Some(parent) = original.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&current, &original)?;
            Ok(())
        }
        "copy" => {
            let copy = security::validate(target_path)?;
            std::fs::remove_file(&copy)?;
            Ok(())
        }
        "delete" => Err(AppError::Forbidden("删除批次暂不支持撤销".into())),
        other => Err(AppError::InvalidInput(format!("未知操作类型: {other}"))),
    }
}
