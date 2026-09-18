//! `files` 表数据仓库：批量写入、查询、分类更新与软删除。
//!
//! 模块划分（原单文件 638 行按职责拆分，各文件 < 300 行，见 `rules/complexity.md`）：
//!   - `read`      读：单条 / 批量 / 条件查询与计数
//!   - `upsert`    写（增量）：批量插入与 upsert，含「(size, mtime) 未变则复用 hash」判定
//!   - `mutate`    写（字段级）：分类与路径更新、软删除、向量化标记清理
//!   - `paths`     按路径前缀批量查询（目录管理 / 批量执行用）
//!   - `embedding` 向量化标记（索引增量的「待建 / 已建」）
//!
//! 全部方法都是「接收外部 `Connection`」的无状态仓库方法；跨模块的 `impl FileRepo`
//! 块共同构成同一类型的方法集合，故调用方（`crate::db::FileRepo::xxx`，全仓 30+ 处）
//! 与 `file_repo_tests.rs` 都无需改动。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::models::FileRecord;

mod embedding;
mod mutate;
mod paths;
mod read;
mod upsert;

/// 单条 SQL 的 `IN (...)` 分块阈值。SQLite 绑定变量上限为 999，留安全余量取 900
/// （T10.1：十万文件扫描时逐条反查是主要瓶颈，改为 900/批的批量快照）。
const CHUNK_IN_PATHS: usize = 900;

/// 增量扫描快照：磁盘元信息与库内既有记录的比对基准（T10.1）。
///
/// 供 [`FileRepo::load_snapshots_by_paths`] 加载后，在锁外做
/// 「(size, mtime) 未变 → 复用 `content_hash`、跳过重算」的增量判定。
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// 库内既有 id（`upsert_batch` 需回写 `id_map`，避免扫描生成的新 uuid 与旧 id 不一致）。
    pub id: String,
    /// 软删除标记（软删记录需复活，不能直接跳过）。
    pub is_deleted: bool,
    /// 既有内容哈希（`None` 表示未计算过，需重算）。
    pub content_hash: Option<String>,
    /// 既有文件大小（字节）。
    pub file_size: i64,
    /// 既有磁盘修改时间（UTC `YYYY-MM-DD HH:MM:SS`；旧迁移行可能为 `None`）。
    pub mtime: Option<String>,
}

/// `files` 表仓库：全部方法接收外部连接，便于事务组合。
pub struct FileRepo;

/// 批量增量写入统计。
#[derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)]
pub struct UpsertResult {
    /// 新增条数。
    pub added: u32,
    /// 更新条数。
    pub updated: u32,
    /// 内容未变化跳过的条数。
    pub skipped: u32,
    /// 传入 id → 入库真实 id 映射（`scan_directory` 回写用；避免同一路径重复扫描时
    /// 扫描生成的新 uuid 与库内旧 id 不一致导致按 id 反查落空）。
    #[serde(skip)]
    #[specta(skip)]
    pub id_map: HashMap<String, String>,
}

/// 将查询行映射为 [`FileRecord`]。
fn map_file_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        path: row.get(1)?,
        file_name: row.get(2)?,
        file_size: row.get(3)?,
        content_hash: row.get(4)?,
        category: row.get(5)?,
        is_deleted: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        mtime: row.get(9)?,
    })
}

#[cfg(test)]
#[path = "../file_repo_tests.rs"]
mod tests;
