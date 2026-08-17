//! `FileMind` 桌面端核心库：文件整理分类器 + RAG 问答。
//!
//! 模块划分：`commands`（IPC 命令）、`db`（`SQLite` + FTS5 数据层）、
//! `security`（路径/密钥/模式切换安全）、`sidecar`（Python 进程管理）、
//! `events`（前端事件负载）。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod security;
pub mod sidecar;

/// 文件信息（IPC 传输视图）。
#[derive(Debug, Serialize, Deserialize, Clone, specta::Type)]
pub struct FileInfo {
    /// 主键（UUID）。
    pub id: String,
    /// 文件绝对路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// 文件大小（字节）。
    pub file_size: u64,
    /// 内容哈希（SHA-256，未计算时为 `None`）。
    pub content_hash: Option<String>,
    /// AI 分类结果。
    pub category: Option<String>,
    /// 发现时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
}

impl From<db::models::FileRecord> for FileInfo {
    fn from(r: db::models::FileRecord) -> Self {
        Self {
            id: r.id,
            path: r.path,
            file_name: r.file_name,
            file_size: u64::try_from(r.file_size).unwrap_or(0),
            content_hash: r.content_hash,
            category: r.category,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// 全局应用状态：由 Tauri 管理并注入各命令。
pub struct AppState {
    /// 数据库句柄（互斥保护，SQLite 连接单线程访问）。
    pub db: Mutex<db::Database>,
}
