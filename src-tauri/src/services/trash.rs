//! 系统回收站封装：把文件移入操作系统回收站（macOS 废纸篓 / Windows
//! 回收站 / freedesktop Trash），替代物理删除实现「删除后可恢复」。
//!
//! 说明：回收站语义由操作系统实现，应用内撤销（`undo_batch`）仍不支持
//! delete——用户可在系统回收站手动恢复。

use std::path::Path;

use crate::error::{AppError, AppResult};

/// 测试串行锁：所有走真实回收站/覆盖目录的删除测试先持锁，避免
/// `FILEMIND_TRASH_DIR` 这一进程级环境变量在并行测试间互相干扰。
#[cfg(test)]
pub(crate) static TEST_TRASH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 把目标路径移入系统回收站（而非物理删除）。
///
/// 测试钩子：环境变量 `FILEMIND_TRASH_DIR` 指向已存在目录时，改为把文件
/// 移入该目录（确定性验证移走行为，避免真实回收站污染与沙箱权限问题）；
/// 未设置时走系统回收站（macOS 废纸篓 / Windows 回收站 / freedesktop Trash）。
///
/// # Errors
///
/// 移入失败（回收站不可用 / 权限不足 / 跨卷异常）时返回 `Io` 错误，
/// 错误消息附带底层原因（不包含敏感路径）。
pub fn move_to_trash(path: &Path) -> AppResult<()> {
    if let Ok(override_dir) = std::env::var("FILEMIND_TRASH_DIR") {
        if !override_dir.is_empty() {
            return move_into_dir(path, Path::new(&override_dir));
        }
    }
    trash::delete(path)
        .map_err(|err| AppError::Io(std::io::Error::other(format!("移入系统回收站失败: {err}"))))
}

/// 把文件移入指定目录（`FILEMIND_TRASH_DIR` 测试钩子的落地实现）。
///
/// # Errors
///
/// 源文件无文件名或 `rename` 失败时返回 `Io` 错误。
fn move_into_dir(source: &Path, dir: &Path) -> AppResult<()> {
    let file_name = source
        .file_name()
        .ok_or_else(|| AppError::InvalidInput("目标路径无文件名".into()))?;
    let target = dir.join(file_name);
    std::fs::rename(source, &target)
        .map_err(|err| AppError::Io(std::io::Error::other(format!("移入回收站目录失败: {err}"))))
}
