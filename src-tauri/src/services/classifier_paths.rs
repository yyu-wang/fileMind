//! 分类目标路径与冲突标记（原 `classifier.rs` 拆出）。
//!
//! 安全：分类输出到扫描根**同级**的收纳目录 `<扫描根名>_已分类`（见 `sibling_output_root`），
//! 避免在扫描目录内新建分类子目录、保持源目录只留待整理文件。目标子目录
//! `categories.target_dir` 经 `security::validate_relative_subpath` 校验后才与收纳根拼接；
//! 目标路径冲突用 `ConflictStrategy::Skip` 提前标记，冲突项由执行层跳过（不覆盖、不改名）。

use std::path::{Path, PathBuf};

use crate::db::models::Category;
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::{self, ConflictStrategy, ConflictType, PlanStatus};

/// 收纳目录命名后缀：扫描根同级目录名为 `<扫描根名>_已分类`。
const SIBLING_SUFFIX: &str = "_已分类";

/// 计算扫描根同级收纳目录路径：`<扫描根父目录>/<扫描根名>_已分类`。
///
/// 分类输出统一落到收纳目录（而非扫描目录内部），保持源目录只留待整理文件。
/// 仅在扫描根名无法确定（如文件系统根 `/`）时返回错误。
///
/// # Errors
///
/// 扫描根为文件系统根、无法取父级目录名时返回 `InvalidInput`。
pub fn sibling_output_root(scan_root: &Path) -> AppResult<PathBuf> {
    let name = scan_root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| AppError::InvalidInput("无法为扫描根生成同级收纳目录名".to_string()))?;
    let parent = scan_root.parent().unwrap_or_else(|| Path::new("/"));
    Ok(parent.join(format!("{name}{SIBLING_SUFFIX}")))
}

/// 拼接目标绝对路径：`收纳根/target_dir/file_name`。
///
/// `target_dir` 为空的分类 → 目标就是 `scan_root/file_name`（不移动，执行时因目标
/// 与源相同被 `Skip` 跳过，仅用于语义占位）。
///
/// # Errors
///
/// `target_dir` 未通过 `validate_relative_subpath` 时返回 `UnsafePath`。
pub(super) fn build_target_path(
    scan_root: &Path,
    output_root: &Path,
    category: &Category,
    file_name: &str,
) -> AppResult<PathBuf> {
    if category.target_dir.trim().is_empty() {
        return Ok(scan_root.join(file_name));
    }
    let sub = security::validate_relative_subpath(&category.target_dir)?;
    Ok(output_root.join(sub).join(file_name))
}

/// 用 `Skip` 策略解析目标冲突：目标已存在 → `Conflict/SameName`（执行时跳过）。
pub(super) fn resolve_conflict(
    target_path: &Path,
    file_name: &str,
    original_path: &Path,
) -> (Option<PathBuf>, PlanStatus, Option<ConflictType>) {
    let target_dir = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("/"));
    conflict_resolver::resolve(file_name, original_path, target_dir, ConflictStrategy::Skip)
}
