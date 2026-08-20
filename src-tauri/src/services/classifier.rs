//! 分类计划生成器：规则引擎 + 启发式兜底 + 待确认。
//!
//! 决策顺序（首个命中即用）：
//!   1. 规则引擎：遍历启用的 `rules`（调用方保证 `priority DESC`），按 `rule_type` 匹配；
//!      命中后反查 `target_category` 对应的分类（不存在则该规则视为未命中，继续）。
//!   2. 启发式：内置「扩展名 → 内置分类名」映射，目标分类须真实存在于 `categories`。
//!   3. 都未命中 → 待确认（`category_name=None`，不生成移动目标）。
//!
//! 安全：目标子目录 `categories.target_dir` 经 `security::validate_relative_subpath`
//! 校验后才与扫描根拼接；目标路径冲突用 `ConflictStrategy::Skip` 提前标记，
//! 冲突项由执行层跳过（不覆盖、不改名）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::db::models::{Category, FileRecord, Rule};
use crate::error::AppResult;
use crate::security;
use crate::services::conflict_resolver::{self, ConflictStrategy, ConflictType, PlanStatus};

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

/// 内置启发式映射：扩展名分组 → 内置分类名。
///
/// 分类名与 `category_repo.rs` 的 `BUILTIN_CATEGORIES` 种子保持一致；
/// 目标分类不在 `categories` 中时启发式不生效（交给待确认）。
const HEURISTIC_EXT_MAP: &[(&str, &[&str])] = &[
    (
        "图片",
        &[
            "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif", "heic",
        ],
    ),
    ("文档", &["pdf"]),
    (
        "视频",
        &["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm", "m4v"],
    ),
    ("音乐", &["mp3", "wav", "flac", "aac", "ogg", "m4a"]),
    ("压缩包", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz"]),
    (
        "办公文档",
        &[
            "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp",
        ],
    ),
    (
        "代码",
        &[
            "txt", "md", "ts", "tsx", "js", "jsx", "py", "rs", "java", "c", "h", "cpp", "css",
            "html", "sh", "sql", "go", "json", "yaml", "yml", "toml", "xml", "log",
        ],
    ),
    ("数据文件", &["csv", "tsv"]),
    ("安装包", &["dmg", "pkg", "exe", "msi", "deb", "rpm", "apk"]),
    ("字体", &["ttf", "otf", "woff", "woff2"]),
    ("电子书", &["epub", "mobi", "azw3"]),
];

/// 单个文件的分类计划项。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ClassifyPlanItem {
    /// 文件 ID。
    pub file_id: String,
    /// 文件名。
    pub file_name: String,
    /// 源文件绝对路径。
    pub original_path: String,
    /// 目标绝对路径（`scan_root/target_dir/file_name`；待确认时等于 `original_path`）。
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
    /// 逐项计划。
    pub items: Vec<ClassifyPlanItem>,
    /// 汇总统计。
    pub stats: ClassifyStats,
}

/// 单文件的分类决策结果。
enum CategoryDecision {
    /// 规则命中（携带目标分类 + 规则名）。
    Rule {
        category: Category,
        rule_name: String,
    },
    /// 启发式命中。
    Heuristic { category: Category },
    /// 无匹配，进入待确认。
    Pending,
}

/// 为一批文件生成分类计划（纯计算，不落盘、不动 DB）。
///
/// `rules` 需已按优先级降序排列（调用方用 `RuleRepo::list_enabled`）；`scan_root`
/// 须为已校验的规范绝对路径；`files` 为待分类的文件记录。
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
    let mut items = Vec::with_capacity(files.len());
    for file in files {
        let decision = decide(&file.file_name, rules, categories);
        let (category_name, rule_source, target_path) = match &decision {
            CategoryDecision::Rule {
                category,
                rule_name,
            } => {
                let target = build_target_path(scan_root, category, &file.file_name)?;
                (
                    Some(category.name.clone()),
                    format!("{RULE_SOURCE_PREFIX}{rule_name}"),
                    target,
                )
            }
            CategoryDecision::Heuristic { category } => {
                let target = build_target_path(scan_root, category, &file.file_name)?;
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

/// 按 规则 → 启发式 → 待确认 顺序判定单个文件的分类。
fn decide(file_name: &str, rules: &[Rule], categories: &[Category]) -> CategoryDecision {
    for rule in rules {
        if match_rule(file_name, rule) {
            if let Some(category) = rule
                .target_category
                .as_deref()
                .and_then(|id| category_by_id(categories, id))
            {
                return CategoryDecision::Rule {
                    category: category.clone(),
                    rule_name: rule.name.clone(),
                };
            }
            // 规则命中但目标分类不存在 → 视为未命中，继续下一规则
        }
    }

    let ext = file_extension(file_name).to_ascii_lowercase();
    if let Some(name) = heuristic_category_name(&ext) {
        if let Some(category) = categories.iter().find(|c| c.name == name) {
            return CategoryDecision::Heuristic {
                category: category.clone(),
            };
        }
    }

    CategoryDecision::Pending
}

/// 判断文件名是否命中单条规则。
///
/// `magic_number` / `size` 需读取文件内容/元数据，属 E4 补充范围，此处不匹配（跳过）。
fn match_rule(file_name: &str, rule: &Rule) -> bool {
    let pattern = rule.pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    match rule.rule_type.as_str() {
        "extension" => {
            let ext = file_extension(file_name).to_ascii_lowercase();
            pattern.split(',').any(|token| {
                let token = token.trim().trim_start_matches('.').to_ascii_lowercase();
                !token.is_empty() && token == ext
            })
        }
        "path_keyword" => file_name.contains(pattern),
        "regex" => Regex::new(pattern).is_ok_and(|re| re.is_match(file_name)),
        _ => false,
    }
}

/// 取文件扩展名（不含点，小写判断由调用方决定；无扩展名返回空串）。
fn file_extension(file_name: &str) -> &str {
    file_name.rfind('.').map_or("", |pos| &file_name[pos + 1..])
}

/// 内置启发式：扩展名 → 分类名。
fn heuristic_category_name(ext: &str) -> Option<&'static str> {
    HEURISTIC_EXT_MAP
        .iter()
        .find(|(_, exts)| exts.contains(&ext))
        .map(|(name, _)| *name)
}

/// 按 id 查分类。
fn category_by_id<'a>(categories: &'a [Category], id: &str) -> Option<&'a Category> {
    categories.iter().find(|c| c.id == id)
}

/// 拼接目标绝对路径：`scan_root/target_dir/file_name`。
///
/// `target_dir` 为空的分类 → 目标就是 `scan_root/file_name`（不移动，执行时因目标
/// 与源相同被 `Skip` 跳过，仅用于语义占位）。
///
/// # Errors
///
/// `target_dir` 未通过 `validate_relative_subpath` 时返回 `UnsafePath`。
fn build_target_path(scan_root: &Path, category: &Category, file_name: &str) -> AppResult<PathBuf> {
    if category.target_dir.trim().is_empty() {
        return Ok(scan_root.join(file_name));
    }
    let sub = security::validate_relative_subpath(&category.target_dir)?;
    Ok(scan_root.join(sub).join(file_name))
}

/// 用 `Skip` 策略解析目标冲突：目标已存在 → `Conflict/SameName`（执行时跳过）。
fn resolve_conflict(
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
                    let target = build_target_path(scan_root, cat, &item.file_name)?;
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
