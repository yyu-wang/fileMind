//! 文件操作命令：扫描、预览、批量执行与撤销。
//!
//! 所有命令做阻塞文件系统操作，通过 `#[tauri::command(async)]`
//! 声明为线程池执行，避免阻塞主线程。

use crate::error::AppResult;
use crate::security;
use crate::FileInfo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_SCAN_DEPTH: u32 = 10;

/// 递归扫描目录并返回文件元信息列表。
///
/// # Errors
///
/// 路径未通过安全校验或文件系统读取失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn scan_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let safe_path = security::validate(&path).map_err(|e| e.to_string())?;
    let files = scan_files_on_disk(&safe_path).map_err(|e| e.to_string())?;
    Ok(files)
}

/// 预览批量文件操作：展示源/目标存在性与冲突，不执行任何变更。
///
/// # Errors
///
/// 源或目标路径未通过安全校验时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn preview_operations(operations: Vec<FileOperation>) -> Result<Vec<OperationPreview>, String> {
    let mut previews = Vec::with_capacity(operations.len());

    for op in operations {
        let source_safe = security::validate(&op.source_path)
            .map_err(|e| format!("FILE-E-002:源路径不安全: {e}"))?;

        let (target_exists, conflict) = if matches!(op.operation_type, OperationType::Delete) {
            (false, false)
        } else {
            let target_safe = security::validate_write_target(&op.target_path)
                .map_err(|e| format!("FILE-E-003:目标路径不安全: {e}"))?;
            let exists = target_safe.exists();
            (exists, exists && source_safe != target_safe)
        };

        previews.push(OperationPreview {
            source_exists: source_safe.exists(),
            target_exists,
            conflict,
            operation: op,
        });
    }

    Ok(previews)
}

/// 逐项执行批量文件操作，返回每项结果与成功/失败计数。
///
/// # Errors
///
/// 仅在内部严重错误时返回；单项失败记录在结果列表中。
#[tauri::command(async)]
#[specta::specta]
pub fn execute_operations(operations: Vec<FileOperation>) -> Result<BatchResult, String> {
    let mut results = Vec::with_capacity(operations.len());
    let mut success_count = 0u32;
    let mut failed_count = 0u32;

    for op in operations {
        let outcome = execute_single(&op);
        let success = outcome.is_ok();
        let error = outcome.err().map(|e| e.to_string());
        if success {
            success_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(OperationResult {
            operation: op,
            success,
            error,
        });
    }

    Ok(BatchResult {
        results,
        success_count,
        failed_count,
    })
}

/// 按批次 ID 撤销已执行的批量操作（待操作日志链实现）。
///
/// # Errors
///
/// 操作日志查询尚未实现时始终返回 `FILE-E-004` 错误。
#[tauri::command(async)]
#[specta::specta]
pub fn undo_batch(batch_id: String) -> Result<BatchResult, String> {
    Err(format!(
        "FILE-E-004:撤销操作尚未实现 (batch_id={batch_id})，需要操作日志查询支持"
    ))
}

/// 从磁盘根目录扫描文件，收集元信息。
fn scan_files_on_disk(root: &Path) -> AppResult<Vec<FileInfo>> {
    let mut files = Vec::new();
    scan_dir_recursive(root, &mut files, 0)?;
    Ok(files)
}

/// 递归遍历目录，超过最大深度时跳过并记录警告。
fn scan_dir_recursive(dir: &Path, files: &mut Vec<FileInfo>, depth: u32) -> AppResult<()> {
    if depth > MAX_SCAN_DEPTH {
        log::warn!("超过最大扫描深度 {MAX_SCAN_DEPTH}，跳过: {}", dir.display());
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            scan_dir_recursive(&path, files, depth + 1)?;
        } else if path.is_file() {
            let metadata = entry.metadata()?;
            let created_at = metadata
                .created()
                .ok()
                .map(format_system_time)
                .unwrap_or_default();
            let updated_at = metadata
                .modified()
                .ok()
                .map(format_system_time)
                .unwrap_or_default();

            files.push(FileInfo {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_string_lossy().to_string(),
                file_name: entry.file_name().to_string_lossy().to_string(),
                file_size: metadata.len(),
                content_hash: None,
                category: None,
                created_at,
                updated_at,
            });
        }
    }

    Ok(())
}

/// 校验路径安全后执行单个文件操作。
fn execute_single(op: &FileOperation) -> AppResult<()> {
    let source = security::validate(&op.source_path)?;

    match op.operation_type {
        OperationType::Move => {
            let target = security::validate_write_target(&op.target_path)?;
            if target.exists() && source != target {
                return Err(crate::error::AppError::UnsafePath(format!(
                    "目标路径已存在且与源路径不同: {}",
                    target.display()
                )));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&source, &target)?;
        }
        OperationType::Rename => {
            let target = security::validate_write_target(&op.target_path)?;
            if target.exists() && source != target {
                return Err(crate::error::AppError::UnsafePath(format!(
                    "目标路径已存在且与源路径不同: {}",
                    target.display()
                )));
            }
            std::fs::rename(&source, &target)?;
        }
        OperationType::Delete => {
            std::fs::remove_file(&source)?;
        }
    }
    Ok(())
}

/// 将系统时间格式化为 `YYYY-MM-DD HH:MM:SS`（UTC）。
fn format_system_time(time: std::time::SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 文件操作类型。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum OperationType {
    /// 移动文件（可跨目录，自动创建父目录）。
    Move,
    /// 重命名文件（同目录内改名）。
    Rename,
    /// 删除文件。
    Delete,
}

/// 单个文件操作描述。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileOperation {
    /// 源文件绝对路径。
    pub source_path: String,
    /// 目标绝对路径（Delete 时忽略）。
    pub target_path: String,
    /// 操作类型。
    pub operation_type: OperationType,
}

/// 单个操作的预览结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationPreview {
    /// 被预览的操作。
    pub operation: FileOperation,
    /// 源路径是否存在。
    pub source_exists: bool,
    /// 目标路径是否存在。
    pub target_exists: bool,
    /// 是否存在冲突（目标已存在且不同于源）。
    pub conflict: bool,
}

/// 单个操作的执行结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationResult {
    /// 已执行的操作。
    pub operation: FileOperation,
    /// 是否成功。
    pub success: bool,
    /// 失败原因（成功时为 `None`）。
    pub error: Option<String>,
}

/// 批量操作汇总结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchResult {
    /// 逐项结果列表。
    pub results: Vec<OperationResult>,
    /// 成功数量。
    pub success_count: u32,
    /// 失败数量。
    pub failed_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn create_temp_file(
        dir: &Path,
        name: &str,
        content: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path)?;
        file.write_all(content.as_bytes())?;
        Ok(path)
    }

    #[test]
    fn test_scan_files_finds_files() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        create_temp_file(tmp.path(), "a.txt", "hello")?;
        create_temp_file(tmp.path(), "b.md", "world")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 2);
        Ok(())
    }

    #[test]
    fn test_scan_files_recursive() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let subdir = tmp.path().join("subdir");
        std::fs::create_dir(&subdir)?;
        create_temp_file(tmp.path(), "top.txt", "top")?;
        create_temp_file(&subdir, "nested.txt", "nested")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.file_name == "nested.txt"));
        Ok(())
    }

    #[test]
    fn test_scan_files_respects_max_depth() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let mut current = tmp.path().to_path_buf();
        for i in 0..(MAX_SCAN_DEPTH + 2) {
            current = current.join(format!("d{i}"));
            std::fs::create_dir(&current)?;
        }
        create_temp_file(&current, "deep.txt", "deep")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert!(files.iter().all(|f| f.file_name != "deep.txt"));
        Ok(())
    }

    #[test]
    fn test_execute_move() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let source = create_temp_file(tmp.path(), "source.txt", "content")?;
        let target = tmp.path().join("target.txt");

        let op = FileOperation {
            source_path: source.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            operation_type: OperationType::Move,
        };

        execute_single(&op)?;
        assert!(!source.exists());
        assert!(target.exists());
        Ok(())
    }

    #[test]
    fn test_execute_delete() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let file = create_temp_file(tmp.path(), "to_delete.txt", "bye")?;

        let op = FileOperation {
            source_path: file.to_string_lossy().to_string(),
            target_path: String::new(),
            operation_type: OperationType::Delete,
        };

        execute_single(&op)?;
        assert!(!file.exists());
        Ok(())
    }

    #[test]
    fn test_execute_move_rejects_existing_target() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let source = create_temp_file(tmp.path(), "source.txt", "content")?;
        let target = create_temp_file(tmp.path(), "target.txt", "existing")?;

        let op = FileOperation {
            source_path: source.to_string_lossy().to_string(),
            target_path: target.to_string_lossy().to_string(),
            operation_type: OperationType::Move,
        };

        let result = execute_single(&op);
        assert!(result.is_err());
        assert!(source.exists());
        assert!(target.exists());
        Ok(())
    }

    #[test]
    fn test_undo_batch_returns_error() {
        let result = undo_batch("test-batch".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_format_system_time() {
        let now = std::time::SystemTime::now();
        let formatted = format_system_time(now);
        assert!(formatted.contains('-'));
        assert!(formatted.contains(':'));
    }
}
