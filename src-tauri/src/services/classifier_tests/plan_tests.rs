//! `generate_plan` 测试：规则命中 / 启发式兜底 / 待确认 / 优先级 / 冲突与统计。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::classifier::{
    aggregate_stats, generate_plan, ClassifyPlanItem, PENDING_SOURCE, RULE_SOURCE_PREFIX,
};
use super::support::*;
use crate::services::conflict_resolver::PlanStatus;

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
