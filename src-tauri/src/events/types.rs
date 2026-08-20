//! Tauri 事件负载类型：与前端通过 `emit`/`listen` 通信。

use serde::{Deserialize, Serialize};

/// 扫描进度事件。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ScanProgressEvent {
    /// 已扫描条数。
    pub scanned: u32,
    /// 预估总数。
    pub total: u32,
    /// 当前正在扫描的路径。
    pub current_path: String,
}

/// 分类进度事件。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ClassifyProgressEvent {
    /// 已处理条数。
    pub processed: u32,
    /// 总条数。
    pub total: u32,
    /// 当前处理的文件名。
    pub current_file: String,
    /// 当前文件分类结果（未分类时为 `None`）。
    pub category: Option<String>,
}

/// 聊天流式事件负载：Rust 代理 Sidecar `/chat/stream` `SSE` 帧后逐帧推送。
///
/// 前端通过 `@tauri-apps/api/event.listen('chat://event')` 订阅；
/// `data` 与 Sidecar `SSE` `data:` 行的 `JSON` 保持一致（`04_API详细规格书` §3.4）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChatEventPayload {
    /// 事件名（`search_start` / `search_result` / `token` / `retry` / `citation` / `done` / `error`）。
    pub event: String,
    /// 事件载荷（与 Sidecar `data:` 行 JSON 一致）。
    pub data: serde_json::Value,
}

/// Sidecar 状态变更事件。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SidecarStatusEvent {
    /// 状态（starting/running/stopped/error）。
    pub status: String,
    /// 附加说明。
    pub message: Option<String>,
}
