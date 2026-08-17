use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod security;
pub mod sidecar;

#[derive(Debug, Serialize, Deserialize, Clone, specta::Type)]
pub struct FileInfo {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub file_size: u64,
    pub content_hash: Option<String>,
    pub category: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<db::models::FileRecord> for FileInfo {
    fn from(r: db::models::FileRecord) -> Self {
        Self {
            id: r.id,
            path: r.path,
            file_name: r.file_name,
            file_size: r.file_size as u64,
            content_hash: r.content_hash,
            category: r.category,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub struct AppState {
    pub db: Mutex<db::Database>,
}
