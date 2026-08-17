use crate::error::AppResult;
use crate::security;
use crate::FileInfo;
use serde::{Deserialize, Serialize};

#[tauri::command]
#[specta::specta]
pub async fn scan_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let safe_path = security::validate(&path).map_err(|e| e.to_string())?;
    let files = scan_files_on_disk(&safe_path).map_err(|e| e.to_string())?;
    Ok(files)
}

#[tauri::command]
#[specta::specta]
pub async fn preview_operations(
    operations: Vec<FileOperation>,
) -> Result<Vec<OperationPreview>, String> {
    let previews: Vec<OperationPreview> = operations
        .into_iter()
        .map(|op| OperationPreview {
            operation: op.clone(),
            source_exists: std::path::Path::new(&op.source_path).exists(),
            target_exists: std::path::Path::new(&op.target_path).exists(),
            conflict: false,
        })
        .collect();
    Ok(previews)
}

#[tauri::command]
#[specta::specta]
pub async fn execute_operations(
    operations: Vec<FileOperation>,
) -> Result<BatchResult, String> {
    let mut results = Vec::new();
    let mut success_count = 0u32;
    let mut failed_count = 0u32;

    for op in operations {
        match execute_single(&op) {
            Ok(_) => {
                success_count += 1;
                results.push(OperationResult {
                    operation: op,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                failed_count += 1;
                results.push(OperationResult {
                    operation: op,
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(BatchResult {
        results,
        success_count,
        failed_count,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn undo_batch(batch_id: String) -> Result<BatchResult, String> {
    todo!("Undo batch implementation pending — requires operation log query")
}

fn scan_files_on_disk(root: &std::path::Path) -> AppResult<Vec<FileInfo>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let metadata = entry.metadata()?;
            files.push(FileInfo {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_string_lossy().to_string(),
                file_name: entry.file_name().to_string_lossy().to_string(),
                file_size: metadata.len(),
                content_hash: None,
                category: None,
                created_at: metadata
                    .created()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default(),
                updated_at: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default(),
            });
        }
    }
    Ok(files)
}

fn execute_single(op: &FileOperation) -> AppResult<()> {
    let source = std::path::Path::new(&op.source_path);
    let target = std::path::Path::new(&op.target_path);

    match op.operation_type {
        OperationType::Move => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(source, target)?;
        }
        OperationType::Rename => {
            std::fs::rename(source, target)?;
        }
        OperationType::Delete => {
            std::fs::remove_file(source)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum OperationType {
    Move,
    Rename,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileOperation {
    pub source_path: String,
    pub target_path: String,
    pub operation_type: OperationType,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationPreview {
    pub operation: FileOperation,
    pub source_exists: bool,
    pub target_exists: bool,
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationResult {
    pub operation: FileOperation,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchResult {
    pub results: Vec<OperationResult>,
    pub success_count: u32,
    pub failed_count: u32,
}
