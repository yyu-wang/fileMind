//! 分类判定引擎：规则 → 启发式 → 待确认（原 `classifier.rs` 拆出）。
//!
//! 决策顺序（首个命中即用）：
//!   1. 规则引擎：遍历启用的 `rules`（调用方保证 `priority DESC`），按 `rule_type` 匹配；
//!      命中后反查 `target_category` 对应的分类（不存在则该规则视为未命中，继续）。
//!   2. 启发式：内置「扩展名 → 内置分类名」映射，目标分类须真实存在于 `categories`。
//!   3. 都未命中 → 待确认（`category_name=None`，不生成移动目标）。
//!
//! 只做「文件名 + 规则 + 分类」→ 决策的纯计算，不碰路径拼接与冲突解析
//! （那部分见 `services::classifier_paths`）。

use regex::Regex;

use crate::db::models::{Category, Rule};

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

/// 单文件的分类决策结果。
pub(super) enum CategoryDecision {
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

/// 按 规则 → 启发式 → 待确认 顺序判定单个文件的分类。
pub(super) fn decide(file_name: &str, rules: &[Rule], categories: &[Category]) -> CategoryDecision {
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
