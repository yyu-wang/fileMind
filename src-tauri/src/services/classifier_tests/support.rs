//! `classifier` 测试公共夹具：分类 / 规则 / 文件记录构造与同级收纳根。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use crate::db::models::{Category, FileRecord, Rule};

use super::super::classifier::sibling_output_root;

pub(super) fn mk_category(id: &str, name: &str, target_dir: &str) -> Category {
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

pub(super) fn mk_rule(
    id: &str,
    name: &str,
    rule_type: &str,
    pattern: &str,
    target: Option<&str>,
) -> Rule {
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

pub(super) fn mk_file(id: &str, name: &str, root: &Path) -> FileRecord {
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
pub(super) fn out_root(root: &Path) -> PathBuf {
    sibling_output_root(root).expect("同级收纳根应可计算")
}
