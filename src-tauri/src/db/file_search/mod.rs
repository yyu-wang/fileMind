//! 文件搜索：FTS5 全文搜索与文件名模糊搜索。
//! 以及 FTS5 内容填充（文件正文入索引，支撑关键词检索）。
//!
//! 模块划分（原单文件 453 行，逼近 Rust 模块 500 行强制阈值，见 `rules/complexity.md`）：
//!   - `search`   FTS5 全文搜索与文件名 LIKE 搜索
//!   - `populate` FTS5 内容填充（正文入索引，幂等「删旧→插新」）
//!   - `query`    查询串清洗与 FTS5 查询构造（注入防御 + CJK 前缀截断）
//!
//! 对外只暴露 [`FileSearch`] 与 [`SearchResult`]；各文件的 `impl FileSearch`
//! 块共同构成同一类型的方法集合，调用方（`crate::db::FileSearch::xxx`）无需改动。

use serde::{Deserialize, Serialize};

use crate::db::models::FileRecord;

/// 搜索入口：全文搜索走 FTS5，文件名搜索走 LIKE。
pub struct FileSearch;

/// 全文搜索命中结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SearchResult {
    /// 命中的文件记录。
    pub file: FileRecord,
    /// bm25 相关度得分（越小越相关）。
    pub score: f64,
}

mod populate;
mod query;
mod search;

#[cfg(test)]
#[path = "../file_search_tests.rs"]
mod tests;
