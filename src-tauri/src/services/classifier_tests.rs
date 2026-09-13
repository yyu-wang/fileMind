//! `classifier` 单元测试：规则引擎 / 启发式 / 待确认 / 优先级 / 冲突标记。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::classifier::{
    aggregate_stats, apply_llm_fallback, generate_plan, sibling_output_root, ClassifyPlanItem,
    HEURISTIC_SOURCE, LLM_SOURCE, NEEDS_REVIEW_SOURCE, PENDING_SOURCE, RULE_SOURCE_PREFIX,
};
use crate::db::models::{Category, FileRecord, Rule};
use crate::services::conflict_resolver::PlanStatus;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn mk_category(id: &str, name: &str, target_dir: &str) -> Category {
    Category {
        id: id.to_string(),
        name: name.to_string(),
        parent_id: None,
        icon: None,
        color: None,
        sort_order: 0,
        is_builtin: true,
        target_dir: target_dir.to_string(),
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    }
}

fn mk_rule(id: &str, name: &str, rule_type: &str, pattern: &str, target: Option<&str>) -> Rule {
    Rule {
        id: id.to_string(),
        name: name.to_string(),
        rule_type: rule_type.to_string(),
        pattern: pattern.to_string(),
        target_category: target.map(str::to_string),
        priority: 100,
        is_enabled: true,
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    }
}

fn mk_file(id: &str, name: &str, root: &Path) -> FileRecord {
    FileRecord {
        id: id.to_string(),
        path: root.join(name).to_string_lossy().to_string(),
        file_name: name.to_string(),
        file_size: 1,
        content_hash: None,
        category: None,
        is_deleted: false,
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
        mtime: None,
    }
}

/// 与分类实现同口径的同级收纳根：`<临时目录名>_已分类`（`sibling_output_root`）。
fn out_root(root: &Path) -> PathBuf {
    sibling_output_root(root).expect("同级收纳根应可计算")
}

#[test]
fn test_extension_rule_matches() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    let rules = vec![mk_rule("r1", "PDF", "extension", "pdf", Some("c1"))];
    let files = vec![mk_file("f1", "report.pdf", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.category_name.as_deref(), Some("财务"));
    assert_eq!(item.rule_source, format!("{RULE_SOURCE_PREFIX}PDF"));
    assert_eq!(
        item.target_path,
        out_root(root.path())
            .join("财务")
            .join("report.pdf")
            .to_string_lossy()
    );
    assert_eq!(item.status, PlanStatus::Ok);
}

#[test]
fn test_extension_rule_case_insensitive_and_dot() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    let rules = vec![mk_rule("r1", "PDF", "extension", ".PDF,  doc", Some("c1"))];
    let files = vec![mk_file("f1", "Report.PDF", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("财务"));
}

#[test]
fn test_path_keyword_rule_matches() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "市场", "市场")];
    let rules = vec![mk_rule("r1", "竞品", "path_keyword", "竞品", Some("c1"))];
    let files = vec![mk_file("f1", "竞品分析_抖音.pdf", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("市场"));
    assert!(items[0].rule_source.starts_with(RULE_SOURCE_PREFIX));
}

#[test]
fn test_regex_rule_matches() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "报表", "报表")];
    let rules = vec![mk_rule(
        "r1",
        "季度",
        "regex",
        r"Q[1-4].*\.xlsx",
        Some("c1"),
    )];
    let files = vec![mk_file("f1", "Q2_报表.xlsx", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("报表"));
}

#[test]
fn test_regex_invalid_pattern_skipped() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "报表", "报表")];
    // 非法正则不应 panic，应视为未命中 → 走待确认
    let rules = vec![mk_rule("r1", "坏正则", "regex", "[unclosed", Some("c1"))];
    let files = vec![mk_file("f1", "anything.pdf", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}

#[test]
fn test_priority_desc_first_hit() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "A类", "A"), mk_category("c2", "B类", "B")];
    // generate_plan 假定调用方按 priority DESC 排序（RuleRepo::list_enabled 保证）：
    // 高优先级规则在前，首个命中即用，后续规则不再生效
    let rules = vec![
        mk_rule("r1", "高优", "path_keyword", "财务", Some("c1")),
        mk_rule("r2", "低优", "extension", "pdf", Some("c2")),
    ];
    let files = vec![mk_file("f1", "财务报告.pdf", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("A类"));
    assert!(items[0].rule_source.contains("高优"));
}

#[test]
fn test_rule_target_category_missing_skipped() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "存在类", "存在")];
    // 规则引用不存在的分类 id → 未命中，继续
    let rules = vec![mk_rule(
        "r1",
        "幽灵",
        "extension",
        "pdf",
        Some("missing-id"),
    )];
    let files = vec![mk_file("f1", "x.pdf", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}

#[test]
fn test_heuristic_fallback() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("builtin-image", "图片", "图片")];
    let files = vec![mk_file("f1", "photo.png", root.path())];

    let items = generate_plan(&files, root.path(), &[], &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("图片"));
    assert_eq!(items[0].rule_source, "heuristic");
    assert_eq!(
        items[0].target_path,
        out_root(root.path())
            .join("图片")
            .join("photo.png")
            .to_string_lossy()
    );
}

#[test]
fn test_heuristic_category_not_seeded_pending() {
    let root = tempfile::tempdir().unwrap();
    // 分类表为空 → 启发式目标不存在 → 待确认
    let files = vec![mk_file("f1", "photo.png", root.path())];

    let items = generate_plan(&files, root.path(), &[], &[]).unwrap();
    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}

#[test]
fn test_unmatched_pending() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "图片", "图片")];
    // .xyz 无启发式映射、无规则 → 待确认
    let files = vec![mk_file("f1", "mystery.xyz", root.path())];

    let items = generate_plan(&files, root.path(), &[], &categories).unwrap();
    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
    assert_eq!(items[0].status, PlanStatus::Ok);
}

#[test]
fn test_target_dir_empty_stays_in_place() {
    let root = tempfile::tempdir().unwrap();
    // target_dir 为空 → 目标 = scan_root/file_name（与源相同），标记冲突由执行层跳过
    // 用规则命中（a.txt 扩展名启发式映射到「代码」而非「原地」，规则兜底保证命中）
    let categories = vec![mk_category("c1", "原地", "")];
    let rules = vec![mk_rule("r1", "原地规则", "extension", "txt", Some("c1"))];
    // 源文件真实落盘：目标 = scan_root/a.txt 与源相同 → 视为冲突
    std::fs::write(root.path().join("a.txt"), b"x").unwrap();
    let files = vec![mk_file("f1", "a.txt", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name.as_deref(), Some("原地"));
    assert_eq!(
        items[0].target_path,
        root.path().join("a.txt").to_string_lossy()
    );
    assert_eq!(items[0].status, PlanStatus::Conflict);
}

#[test]
fn test_conflict_marked_when_target_exists() {
    let root = tempfile::tempdir().unwrap();
    // 冲突检测基于同级收纳根下的目标：目标已存在 → 标记 Conflict（Skip）
    let out = out_root(root.path());
    std::fs::create_dir_all(out.join("图片")).unwrap();
    std::fs::write(out.join("图片").join("photo.png"), b"existing").unwrap();
    let categories = vec![mk_category("builtin-image", "图片", "图片")];
    let files = vec![mk_file("f1", "photo.png", root.path())];

    let items = generate_plan(&files, root.path(), &[], &categories).unwrap();
    assert_eq!(items[0].status, PlanStatus::Conflict);
}

#[test]
fn test_unsafe_target_dir_rejected() {
    let root = tempfile::tempdir().unwrap();
    // 分类 target_dir 含路径穿越 → generate_plan 报错，不生成非法目标
    // 用规则命中（启发式映射不到「越权」，规则确保走到 build_target_path）
    let categories = vec![mk_category("c1", "越权", "../secret")];
    let rules = vec![mk_rule("r1", "越权规则", "extension", "txt", Some("c1"))];
    let files = vec![mk_file("f1", "a.txt", root.path())];

    let result = generate_plan(&files, root.path(), &rules, &categories);
    assert!(result.is_err());
}

#[test]
fn test_magic_number_rule_skipped() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "二进制", "二进制")];
    // magic_number 规则 E4 才实现，本任务跳过 → 待确认
    let rules = vec![mk_rule(
        "r1",
        "PNG头",
        "magic_number",
        "89504e47",
        Some("c1"),
    )];
    let files = vec![mk_file("f1", "a.png", root.path())];

    let items = generate_plan(&files, root.path(), &rules, &categories).unwrap();
    assert_eq!(items[0].category_name, None);
}

#[test]
fn test_aggregate_stats_counts() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("builtin-image", "图片", "图片")];
    let rules = vec![mk_rule(
        "r1",
        "PDF",
        "extension",
        "pdf",
        Some("builtin-image"),
    )];
    let files = vec![
        mk_file("f1", "a.png", root.path()), // heuristic
        mk_file("f2", "b.pdf", root.path()), // rule
        mk_file("f3", "c.xyz", root.path()), // pending
    ];
    let items: Vec<ClassifyPlanItem> =
        generate_plan(&files, root.path(), &rules, &categories).unwrap();
    let stats = aggregate_stats(&items);

    assert_eq!(stats.total, 3);
    assert_eq!(stats.categorized, 2);
    assert_eq!(stats.pending, 1);
    assert_eq!(stats.by_rule, 1);
    assert_eq!(stats.by_heuristic, 1);
}

// ----------------------------------------------------------------------
// T6.11 apply_llm_fallback 测试
// ----------------------------------------------------------------------

/// 构造一个 pending 项（规则/启发式未命中）。
fn mk_pending_item(id: &str, name: &str, root: &Path) -> ClassifyPlanItem {
    ClassifyPlanItem {
        file_id: id.to_string(),
        file_name: name.to_string(),
        original_path: root.join(name).to_string_lossy().to_string(),
        target_path: root.join(name).to_string_lossy().to_string(),
        category_name: None,
        rule_source: PENDING_SOURCE.to_string(),
        status: PlanStatus::Ok,
        conflict_type: None,
    }
}

#[test]
fn test_llm_fallback_classified_merges() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    let mut items = vec![mk_pending_item("f1", "report.txt", root.path())];
    let llm_map: HashMap<String, (String, String)> = HashMap::from([(
        "report.txt".to_string(),
        ("财务".to_string(), "classified".to_string()),
    )]);

    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    assert_eq!(items[0].category_name.as_deref(), Some("财务"));
    assert_eq!(items[0].rule_source, LLM_SOURCE);
    assert_eq!(
        items[0].target_path,
        out_root(root.path())
            .join("财务")
            .join("report.txt")
            .to_string_lossy()
    );
    assert_eq!(items[0].status, PlanStatus::Ok);
}

#[test]
fn test_llm_fallback_needs_review_marked() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    let mut items = vec![mk_pending_item("f1", "mystery.txt", root.path())];
    let llm_map: HashMap<String, (String, String)> = HashMap::from([(
        "mystery.txt".to_string(),
        ("财务".to_string(), "needs_review".to_string()),
    )]);

    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    // 低置信度：保持未分类，但标记待人工确认
    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, NEEDS_REVIEW_SOURCE);
    assert_eq!(items[0].target_path, items[0].original_path);
}

#[test]
fn test_llm_fallback_unclassified_keeps_pending() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    let mut items = vec![mk_pending_item("f1", "mystery.txt", root.path())];
    let llm_map: HashMap<String, (String, String)> = HashMap::from([(
        "mystery.txt".to_string(),
        ("未分类".to_string(), "unclassified".to_string()),
    )]);

    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}

#[test]
fn test_llm_fallback_unknown_category_keeps_pending() {
    let root = tempfile::tempdir().unwrap();
    // LLM 返回的分类不在 categories → 不移动，防幻影分类
    let categories = vec![mk_category("c1", "财务", "财务")];
    let mut items = vec![mk_pending_item("f1", "mystery.txt", root.path())];
    let llm_map: HashMap<String, (String, String)> = HashMap::from([(
        "mystery.txt".to_string(),
        ("不存在分类".to_string(), "classified".to_string()),
    )]);

    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    assert_eq!(items[0].category_name, None);
    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}

#[test]
fn test_llm_fallback_skips_already_categorized() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("builtin-image", "图片", "图片")];
    // 启发式已命中 → 不应被 LLM 结果覆盖
    let files = vec![mk_file("f1", "photo.png", root.path())];
    let mut items: Vec<ClassifyPlanItem> =
        generate_plan(&files, root.path(), &[], &categories).unwrap();
    assert_eq!(items[0].rule_source, HEURISTIC_SOURCE);

    // LLM 想把它分到「财务」→ 应被忽略（只处理 pending）
    let llm_map: HashMap<String, (String, String)> = HashMap::from([(
        "photo.png".to_string(),
        ("财务".to_string(), "classified".to_string()),
    )]);
    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    assert_eq!(items[0].category_name.as_deref(), Some("图片"));
    assert_eq!(items[0].rule_source, HEURISTIC_SOURCE);
}

#[test]
fn test_llm_fallback_no_match_keeps_pending() {
    let root = tempfile::tempdir().unwrap();
    let categories = vec![mk_category("c1", "财务", "财务")];
    // llm_map 里没有该文件 → 保持 pending
    let mut items = vec![mk_pending_item("f1", "mystery.txt", root.path())];
    let llm_map: HashMap<String, (String, String)> = HashMap::new();

    apply_llm_fallback(&mut items, &llm_map, root.path(), &categories).unwrap();

    assert_eq!(items[0].rule_source, PENDING_SOURCE);
}
