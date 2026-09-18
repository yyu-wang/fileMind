//! T6.11 `apply_llm_fallback` 测试：合并 / 待人工确认 / 未分类 / 幻影分类 / 跳过已分类。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::Path;

use super::super::classifier::{
    apply_llm_fallback, generate_plan, ClassifyPlanItem, HEURISTIC_SOURCE, LLM_SOURCE,
    NEEDS_REVIEW_SOURCE, PENDING_SOURCE,
};
use super::support::*;
use crate::services::conflict_resolver::PlanStatus;

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
