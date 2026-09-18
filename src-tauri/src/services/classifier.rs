//! 分类计划生成器：规则引擎 + 启发式兜底 + 待确认。
//!
//! 决策顺序（首个命中即用）：
//!   1. 规则引擎：遍历启用的 `rules`（调用方保证 `priority DESC`），按 `rule_type` 匹配；
//!      命中后反查 `target_category` 对应的分类（不存在则该规则视为未命中，继续）。
//!   2. 启发式：内置「扩展名 → 内置分类名」映射，目标分类须真实存在于 `categories`。
//!   3. 都未命中 → 待确认（`category_name=None`，不生成移动目标）。
//!
//! 安全：分类输出到扫描根**同级**的收纳目录 `<扫描根名>_已分类`（见 `sibling_output_root`），
//! 避免在扫描目录内新建分类子目录、保持源目录只留待整理文件。目标子目录
//! `categories.target_dir` 经 `security::validate_relative_subpath` 校验后才与收纳根拼接；
//! 目标路径冲突用 `ConflictStrategy::Skip` 提前标记，冲突项由执行层跳过（不覆盖、不改名）。
//!
//! 拆分（原单文件 407 行，逼近 Rust 模块 500 行强制阈值）：
//!   - `services/classifier_engine.rs` 判定引擎（规则 / 启发式 / 待确认）
//!   - `services/classifier_paths.rs`  目标路径拼接与冲突标记
//!
//! 本文件留对外契约（来源标签 + 三个 specta 类型）与编排（生成计划 / LLM 兜底合并 / 统计）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::models::{Category, FileRecord, Rule};
use crate::error::AppResult;
use crate::services::classifier_engine::{decide, CategoryDecision};
use crate::services::classifier_paths::{build_target_path, resolve_conflict};
use crate::services::conflict_resolver::{ConflictType, PlanStatus};

/// 规则匹配来源标签前缀（`rule:<规则名>`），前端据此区分 规则/启发式/待确认。
pub(crate) const RULE_SOURCE_PREFIX: &str = "rule:";
/// 启发式匹配来源标签。
pub(crate) const HEURISTIC_SOURCE: &str = "heuristic";
/// 待确认来源标签。
pub(crate) const PENDING_SOURCE: &str = "pending";
/// LLM 兜底命中来源标签（T6.11：sidecar /classify 补全）。
pub(crate) const LLM_SOURCE: &str = "llm";
/// 待人工确认来源标签（T6.11：LLM 兜底命中但置信度低于阈值）。
pub(crate) const NEEDS_REVIEW_SOURCE: &str = "needs_review";

// 收纳根计算已移到 classifier_paths，此处再导出以保持既有调用路径
// （commands/classify.rs 与 classifier_tests.rs 的 `classifier::sibling_output_root`）。
pub use crate::services::classifier_paths::sibling_output_root;

/// 单个文件的分类计划项。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ClassifyPlanItem {
    /// 文件 ID。
    pub file_id: String,
    /// 文件名。
    pub file_name: String,
    /// 源文件绝对路径。
    pub original_path: String,
    /// 目标绝对路径（`收纳根/target_dir/file_name`；待确认时等于 `original_path`）。
    pub target_path: String,
    /// 落入的分类名（`None`=待确认）。
    pub category_name: Option<String>,
    /// 来源标签：`rule:<规则名>` / `heuristic` / `pending`。
    pub rule_source: String,
    /// 计划状态（`Ok` 可执行 / `Conflict` 目标已存在需跳过 / `Error` 不可执行）。
    pub status: PlanStatus,
    /// 冲突类型（仅在 `status=Conflict` 时有值）。
    pub conflict_type: Option<ConflictType>,
}

/// 分类统计（前端预览面板按 规则/启发式/待确认 展示占比）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct ClassifyStats {
    /// 文件总数。
    #[specta(type = specta_typescript::Number)]
    pub total: u32,
    /// 已分配分类（规则 + 启发式命中）。
    #[specta(type = specta_typescript::Number)]
    pub categorized: u32,
    /// 待确认数。
    #[specta(type = specta_typescript::Number)]
    pub pending: u32,
    /// 规则命中数。
    #[specta(type = specta_typescript::Number)]
    pub by_rule: u32,
    /// 启发式命中数。
    #[specta(type = specta_typescript::Number)]
    pub by_heuristic: u32,
}

/// 分类预览响应（`classify_preview` 命令返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ClassifyPreview {
    /// 批次 ID（供执行阶段 `execute_operations` 接力，共享链式撤销）。
    pub batch_id: String,
    /// 收纳根绝对路径（扫描根同级的 `<扫描根名>_已分类`，前端「目标结构」展示与手动分类拼接用）。
    pub output_root: String,
    /// 逐项计划。
    pub items: Vec<ClassifyPlanItem>,
    /// 汇总统计。
    pub stats: ClassifyStats,
}

/// 为一批文件生成分类计划（纯计算，不落盘、不动 DB）。
///
/// `rules` 需已按优先级降序排列（调用方用 `RuleRepo::list_enabled`）；`scan_root`
/// 须为已校验的规范绝对路径。目标目录基于扫描根**同级**收纳根（`sibling_output_root`）
/// 拼接，由 `classify_preview` 命令层的 `output_root` 保持一致；`files` 为待分类文件记录。
///
/// # Errors
///
/// 目标子路径校验失败（含路径穿越/非法分段）时返回 `UnsafePath`。
pub fn generate_plan(
    files: &[FileRecord],
    scan_root: &Path,
    rules: &[Rule],
    categories: &[Category],
) -> AppResult<Vec<ClassifyPlanItem>> {
    let output_root = sibling_output_root(scan_root)?;
    let mut items = Vec::with_capacity(files.len());
    for file in files {
        let decision = decide(&file.file_name, rules, categories);
        let (category_name, rule_source, target_path) = match &decision {
            CategoryDecision::Rule {
                category,
                rule_name,
            } => {
                let target = build_target_path(scan_root, &output_root, category, &file.file_name)?;
                (
                    Some(category.name.clone()),
                    format!("{RULE_SOURCE_PREFIX}{rule_name}"),
                    target,
                )
            }
            CategoryDecision::Heuristic { category } => {
                let target = build_target_path(scan_root, &output_root, category, &file.file_name)?;
                (
                    Some(category.name.clone()),
                    HEURISTIC_SOURCE.to_string(),
                    target,
                )
            }
            CategoryDecision::Pending => {
                (None, PENDING_SOURCE.to_string(), PathBuf::from(&file.path))
            }
        };

        let (status, conflict_type) = match &decision {
            CategoryDecision::Pending => (PlanStatus::Ok, None),
            CategoryDecision::Rule { .. } | CategoryDecision::Heuristic { .. } => {
                let (_, status, conflict_type) =
                    resolve_conflict(&target_path, &file.file_name, Path::new(&file.path));
                (status, conflict_type)
            }
        };

        items.push(ClassifyPlanItem {
            file_id: file.id.clone(),
            file_name: file.file_name.clone(),
            original_path: file.path.clone(),
            target_path: target_path.to_string_lossy().to_string(),
            category_name,
            rule_source,
            status,
            conflict_type,
        });
    }
    Ok(items)
}

/// 聚合分类统计。
#[must_use]
pub fn aggregate_stats(items: &[ClassifyPlanItem]) -> ClassifyStats {
    let mut stats = ClassifyStats {
        total: u32::try_from(items.len()).unwrap_or(u32::MAX),
        ..ClassifyStats::default()
    };
    for item in items {
        match item.rule_source.as_str() {
            PENDING_SOURCE | NEEDS_REVIEW_SOURCE => stats.pending += 1,
            HEURISTIC_SOURCE => {
                stats.by_heuristic += 1;
                stats.categorized += 1;
            }
            _ => {
                stats.by_rule += 1;
                stats.categorized += 1;
            }
        }
    }
    stats
}

/// 把 LLM 兜底结果合并进 plan（T6.11，纯函数便于单测）。
///
/// `llm_map`：`file_name → (category, status)`，来自 sidecar `/classify` 响应
/// （该响应只回显 `file_name`，不携带 `file_id`，故按文件名反查）。
/// status 语义对齐 sidecar：`classified` / `needs_review` / `unclassified`。
///
/// 合并规则（只处理当前为 `pending` 的项）：
///   - `classified`：分类存在于 `categories` → 归入该分类（`rule_source="llm"`），
///     重新计算目标路径与冲突标记；分类不存在 → 保持待确认（不移动）。
///   - `needs_review`：保持未分类，但标记 `rule_source="needs_review"`（待人工确认）。
///   - `unclassified` / 未命中 / 未知状态：保持原 `pending`。
///
/// # Errors
///
/// 目标子路径校验失败时返回 `UnsafePath`（与 `generate_plan` 一致）。
pub(crate) fn apply_llm_fallback(
    items: &mut [ClassifyPlanItem],
    llm_map: &HashMap<String, (String, String)>,
    scan_root: &Path,
    categories: &[Category],
) -> AppResult<()> {
    let output_root = sibling_output_root(scan_root)?;
    for item in items.iter_mut() {
        // 只对「规则 + 启发式均未命中」的项做 LLM 兜底合并
        if item.rule_source != PENDING_SOURCE {
            continue;
        }
        let Some((category, status)) = llm_map.get(&item.file_name) else {
            continue;
        };
        match status.as_str() {
            "classified" => {
                // LLM 返回的分类必须真实存在于 categories，否则不移动（防幻影分类）
                if let Some(cat) = categories.iter().find(|c| c.name == *category) {
                    let target = build_target_path(scan_root, &output_root, cat, &item.file_name)?;
                    let (_, plan_status, conflict_type) =
                        resolve_conflict(&target, &item.file_name, Path::new(&item.original_path));
                    item.category_name = Some(cat.name.clone());
                    item.rule_source = LLM_SOURCE.to_string();
                    item.target_path = target.to_string_lossy().to_string();
                    item.status = plan_status;
                    item.conflict_type = conflict_type;
                }
            }
            "needs_review" => {
                // LLM 低置信度 → 标记待人工确认（仍不移动，留给 T6.12 手动处理）
                item.rule_source = NEEDS_REVIEW_SOURCE.to_string();
            }
            // "unclassified" 或未知状态：保持原 pending，不处理
            _ => {}
        }
    }
    Ok(())
}
