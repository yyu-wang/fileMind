//! `FileMind` 桌面端核心库：文件整理分类器 + RAG 问答。
//!
//! 模块划分：`commands`（IPC 命令）、`db`（`SQLite` + FTS5 数据层）、
//! `security`（路径/密钥/模式切换安全）、`sidecar`（Python 进程管理）、
//! `events`（前端事件负载）。

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use crate::sidecar::SidecarManager;

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
    /// Sidecar 进程管理器（互斥保护）：
    /// 由 `main.rs` 启动 + 握手后放入，生命周期内由 `watchdog` 与 `on_exit` 共同访问。
    pub sidecar_manager: Mutex<SidecarManager>,
    /// Sidecar 握手后的 PSK；握手前为 `None`，握手/重启成功后更新为新密钥。
    /// 安全映射：`S-01`（Sidecar 端口冒充）。
    pub sidecar_psk: Mutex<Option<Vec<u8>>>,
    /// Sidecar 二进制绝对路径（互斥保护）：
    /// 启动阶段（main）解析 dev 路径占位写入，`setup` 回调内若命中 bundle 模式则
    /// 替换为 Tauri resources 路径。各模块统一从此字段读实际执行路径。
    pub sidecar_binary: Mutex<std::path::PathBuf>,
    /// 请求序号（单调递增），用于 Sidecar 防重放校验。
    /// Sidecar 重启时需重置为 `0`（新 Sidecar 端序列号从 0 开始）。
    /// 安全映射：`T-01`（Sidecar 通信篡改）。
    pub request_seq: AtomicU64,
    /// 历史累计崩溃重启次数（用于状态面板 + 排障）。
    /// 与 `recent_restarts`（`CrashLoop` 窗口）是两个独立计数器。
    pub sidecar_restart_count: AtomicU64,
}
