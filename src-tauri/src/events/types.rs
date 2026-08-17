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

/// 对话流式 Token 事件。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChatTokenEvent {
    /// 本帧 Token 文本。
    pub token: String,
    /// 是否为最后一帧。
    pub is_final: bool,
}

/// Sidecar 状态变更事件。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SidecarStatusEvent {
    /// 状态（starting/running/stopped/error）。
    pub status: String,
    /// 附加说明。
    pub message: Option<String>,
}
