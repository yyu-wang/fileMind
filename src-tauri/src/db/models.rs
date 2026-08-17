use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileRecord {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub file_size: i64,
    pub content_hash: Option<String>,
    pub category: Option<String>,
    pub is_deleted: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationLog {
    pub id: String,
    pub batch_id: String,
    pub operation_type: String,
    pub source_path: String,
    pub target_path: String,
    pub status: String,
    pub prev_hash: String,
    pub current_hash: String,
    pub created_at: String,
}
