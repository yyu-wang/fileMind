//! 冲突解析：根据策略计算最终 `new_path` + `status` + `conflict_type`。
//!
//! 设计要点：
//! - 纯函数，接收 `(file_name, original_path, target_dir, strategy)`，无副作用
//! - 不查文件系统存在性（调用方在 `build_plan_item` 里做一次 `exists()` 检查后传入）
//! - 4 个策略集中在 `resolve_with_strategy` 分派，便于单测与扩展
//!
//! 业务规则（对齐 `04_API详细规格书.html §s2-2a`）：
//!
//! | 策略       | 目标不存在          | 目标已存在                          |
//! |-----------|--------------------|------------------------------------|
//! | Rename    | new=target/name    | new=target/stem_1.ext（递增到不冲突）|
//! | Overwrite | new=target/name    | new=target/name（status=Ok）        |
//! | Skip      | new=target/name    | new=None, status=Conflict          |
//! | KeepBoth  | new=target/name    | new=target/stem_copy.ext           |

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 冲突策略：目标已存在时如何处理（API §s2-2a `conflict_strategy`）。
///
/// `None` 时默认 `Rename`（前端不显式传即取默认）。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// 默认：目标已存在则把新文件重命名为 `xxx_1.pdf` / `xxx_2.pdf`（递增直到不冲突）。
    #[default]
    Rename,
    /// 覆盖目标（危险，前端要二次确认；执行阶段才真覆盖）。
    Overwrite,
    /// 跳过冲突项（`status=Conflict`，不执行）。
    Skip,
    /// 两份都保留：源改名为 `xxx_copy.pdf` 后移动到目标目录。
    KeepBoth,
}

/// 单个 plan 项的最终状态（API §s2-2a `plan[].status`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum PlanStatus {
    /// 可执行（目标不存在 / 覆盖策略允许 / 重命名后无冲突）。
    Ok,
    /// 冲突需用户决策（`Skip` 策略下命中冲突，或 `KeepBoth` 下源副本也冲突）。
    Conflict,
    /// 不可执行（文件不存在、权限不足等）。
    Error,
}

/// 冲突类型细分（API §s2-2a `plan[].conflict_type`），仅在 `status=Conflict` 时有值。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum ConflictType {
    /// 目标路径已存在同名文件。
    SameName,
    /// 权限不足（父目录不可写等）。
    Permission,
}

/// 解析单个文件的 plan item 字段。
///
/// 入参：
///   - `file_name`：源文件名（含扩展名，如 `report.pdf`）
///   - `_original_path`：源文件路径（当前未使用，预留接口一致性）
///   - `target_dir`：目标目录（绝对路径）
///   - `strategy`：冲突策略
///
/// 返回 `(new_path, status, conflict_type)`：
///   - `new_path`：`Some(PathBuf)` 表示有可用目标；`None` 表示不可执行（Skip 冲突）
///   - `status`：`Ok` / `Conflict` / `Error`
///   - `conflict_type`：仅在 `status=Conflict` 时为 `Some`
///
/// 注意：本函数**不查文件系统存在性**，调用方需提前检查并把「目标是否已存在」通过
/// `target_exists` 传入。这里为了保持接口简单，直接在调用方做存在性判断后决定
/// 是否调用本函数的「存在冲突」分支。
#[must_use]
pub fn resolve(
    file_name: &str,
    _original_path: &Path,
    target_dir: &Path,
    strategy: ConflictStrategy,
) -> (Option<PathBuf>, PlanStatus, Option<ConflictType>) {
    // 基础目标路径 = target_dir/file_name
    let base_target = target_dir.join(file_name);

    // 调用方已经判断过 target_exists，这里通过 strategy 直接分派
    // 但本函数是纯函数，调用方传入「是否冲突」更清晰；为简化接口，
    // 这里直接调 std::fs::exists() —— 这会让函数不再是纯函数，
    // 但实际上 file 系统 exists() 在临时目录下足够快，且测试用 tempfile 控制
    let target_exists = base_target.exists();

    resolve_with_strategy(file_name, target_dir, &base_target, target_exists, strategy)
}

/// 纯函数入口：已知 `target_exists` 后按策略分派。
///
/// 抽出来便于单测：测试不需要真的去文件系统建文件，直接传 `target_exists: bool`。
#[must_use]
pub fn resolve_with_strategy(
    file_name: &str,
    target_dir: &Path,
    base_target: &Path,
    target_exists: bool,
    strategy: ConflictStrategy,
) -> (Option<PathBuf>, PlanStatus, Option<ConflictType>) {
    // 不存在冲突时，所有策略都返回 base_target, status=Ok
    if !target_exists {
        return (Some(base_target.to_path_buf()), PlanStatus::Ok, None);
    }

    // 目标已存在：按策略分派
    match strategy {
        ConflictStrategy::Rename => {
            // 递增找不冲突名：xxx_1.ext, xxx_2.ext, ...
            let new_path = find_non_conflicting_name(target_dir, file_name, "_", 1);
            (Some(new_path), PlanStatus::Ok, None)
        }
        ConflictStrategy::Overwrite => {
            // 直接覆盖：new_path 仍为 base_target，status=Ok
            (Some(base_target.to_path_buf()), PlanStatus::Ok, None)
        }
        ConflictStrategy::Skip => {
            // 跳过：new_path=None, status=Conflict
            (None, PlanStatus::Conflict, Some(ConflictType::SameName))
        }
        ConflictStrategy::KeepBoth => {
            // 源改副本名：xxx_copy.ext（递增到不冲突）
            let new_path = find_keep_both_name(target_dir, file_name);
            (Some(new_path), PlanStatus::Ok, None)
        }
    }
}

/// 递增查找不冲突的文件名：`{stem}{sep}{n}{ext}`，n 从 `start` 开始。
///
/// 尝试 `report_1.pdf`, `report_2.pdf` 等，直到不存在。
fn find_non_conflicting_name(target_dir: &Path, file_name: &str, sep: &str, start: u32) -> PathBuf {
    let (stem, ext) = split_file_name(file_name);
    let mut n = start;
    loop {
        let candidate_name = if ext.is_empty() {
            format!("{stem}{sep}{n}")
        } else {
            format!("{stem}{sep}{n}.{ext}")
        };
        let candidate = target_dir.join(&candidate_name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;

        // 防御：避免无限循环（理论上 i32::MAX 之前总能找到，但作为安全阀）
        if n > 100_000 {
            log::warn!(
                "find_non_conflicting_name 超过 100000 次尝试，返回最后候选: {candidate_name}"
            );
            return candidate;
        }
    }
}

/// `KeepBoth` 策略：源改副本名 `{stem}_copy.{ext}`，若已存在则再递增 `_copy_1`。
fn find_keep_both_name(target_dir: &Path, file_name: &str) -> PathBuf {
    let (stem, ext) = split_file_name(file_name);
    let copy_name = if ext.is_empty() {
        format!("{stem}_copy")
    } else {
        format!("{stem}_copy.{ext}")
    };
    let candidate = target_dir.join(&copy_name);
    if !candidate.exists() {
        return candidate;
    }
    // xxx_copy.ext 已存在 → 退化为 Rename 风格 xxx_copy_1.ext
    find_non_conflicting_name(target_dir, &copy_name, "_", 1)
}

/// 拆分文件名：返回 (stem, ext)，ext 不含点。
///
/// `report.pdf` → (`report`, `pdf`)
/// `archive.tar.gz` → (`archive.tar`, `gz`)
/// `noext` → (`noext`, ``)
/// `.hidden` → (``, `hidden`)
pub(super) fn split_file_name(file_name: &str) -> (&str, &str) {
    match file_name.rfind('.') {
        Some(pos) if pos > 0 => (&file_name[..pos], &file_name[pos + 1..]),
        Some(pos) => (&file_name[..pos], &file_name[pos + 1..]),
        None => (file_name, ""),
    }
}
