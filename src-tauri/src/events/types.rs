use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ScanProgressEvent {
    pub scanned: u32,
    pub total: u32,
    pub current_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ClassifyProgressEvent {
    pub processed: u32,
    pub total: u32,
    pub current_file: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChatTokenEvent {
    pub token: String,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SidecarStatusEvent {
    pub status: String,
    pub message: Option<String>,
}
