//! 内置分类种子与幂等写入（原 `category_repo.rs` 拆出）。

use rusqlite::{params, Connection};

use super::CategoryRepo;
use crate::error::AppResult;

/// 内置分类种子：`is_builtin=1` 的默认分类（启发式兜底的目标分类）。
///
/// `id` 用稳定 slug，便于幂等 `INSERT OR IGNORE`；`name` 与分类器启发式映射表
/// `services/classifier_engine.rs` 的 `HEURISTIC_EXT_MAP` 值保持一致。
pub(super) struct BuiltinCategorySeed {
    id: &'static str,
    name: &'static str,
    target_dir: &'static str,
    icon: &'static str,
    color: &'static str,
    sort_order: i64,
}

pub(super) const BUILTIN_CATEGORIES: &[BuiltinCategorySeed] = &[
    BuiltinCategorySeed {
        id: "builtin-image",
        name: "图片",
        target_dir: "图片",
        icon: "image",
        color: "accent",
        sort_order: 10,
    },
    BuiltinCategorySeed {
        id: "builtin-document",
        name: "文档",
        target_dir: "文档",
        icon: "file-text",
        color: "accent2",
        sort_order: 20,
    },
    BuiltinCategorySeed {
        id: "builtin-video",
        name: "视频",
        target_dir: "视频",
        icon: "video",
        color: "accent3",
        sort_order: 30,
    },
    BuiltinCategorySeed {
        id: "builtin-music",
        name: "音乐",
        target_dir: "音乐",
        icon: "music",
        color: "accent2",
        sort_order: 40,
    },
    BuiltinCategorySeed {
        id: "builtin-archive",
        name: "压缩包",
        target_dir: "压缩包",
        icon: "archive",
        color: "warn",
        sort_order: 50,
    },
    BuiltinCategorySeed {
        id: "builtin-office",
        name: "办公文档",
        target_dir: "办公文档",
        icon: "briefcase",
        color: "accent2",
        sort_order: 60,
    },
    BuiltinCategorySeed {
        id: "builtin-code",
        name: "代码",
        target_dir: "代码",
        icon: "code",
        color: "accent",
        sort_order: 70,
    },
    BuiltinCategorySeed {
        id: "builtin-data",
        name: "数据文件",
        target_dir: "数据文件",
        icon: "database",
        color: "accent2",
        sort_order: 80,
    },
    BuiltinCategorySeed {
        id: "builtin-installer",
        name: "安装包",
        target_dir: "安装包",
        icon: "package",
        color: "warn",
        sort_order: 90,
    },
    BuiltinCategorySeed {
        id: "builtin-font",
        name: "字体",
        target_dir: "字体",
        icon: "type",
        color: "accent",
        sort_order: 100,
    },
    BuiltinCategorySeed {
        id: "builtin-ebook",
        name: "电子书",
        target_dir: "电子书",
        icon: "book",
        color: "accent3",
        sort_order: 110,
    },
];

impl CategoryRepo {
    /// 幂等写入内置分类（`INSERT OR IGNORE`，按稳定 id 去重）。
    ///
    /// 首次启动时保证启发式兜底有目标分类可用；已存在则跳过，不改动用户数据。
    /// 返回实际新增条数。
    ///
    /// # Errors
    ///
    /// 写入失败时返回数据库错误。
    pub fn seed_builtin_categories(conn: &Connection) -> AppResult<usize> {
        let mut inserted = 0usize;
        for seed in BUILTIN_CATEGORIES {
            let affected = conn.execute(
                "INSERT OR IGNORE INTO categories
                 (id, name, parent_id, icon, color, sort_order, is_builtin, target_dir)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, 1, ?6)",
                params![
                    seed.id,
                    seed.name,
                    seed.icon,
                    seed.color,
                    seed.sort_order,
                    seed.target_dir,
                ],
            )?;
            inserted += affected;
        }
        Ok(inserted)
    }
}
