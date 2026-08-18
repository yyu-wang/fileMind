//! 文件操作命令：扫描、预览、批量执行与撤销。
//!
//! 所有命令做阻塞文件系统操作，通过 `#[tauri::command(async)]`
//! 声明为线程池执行，避免阻塞主线程。

use crate::db::models::FileRecord;
use crate::db::FileRepo;
use crate::error::AppResult;
use crate::security;
use crate::services::hash_service::compute_file_hash;
use crate::{AppState, FileInfo};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::convert::From;
use std::path::Path;

const MAX_SCAN_DEPTH: u32 = 10;

/// 递归扫描目录并返回文件元信息列表（含 `content_hash`），并 `upsert` 到 `SQLite`。
///
/// 行为：
///   1. 路径安全校验（`security::validate`）
///   2. 递归扫描目录 + 读取元信息 + 计算 SHA-256 hash（单文件失败退化为 None，不阻塞全量扫描）
///   3. 把扫描结果 `upsert` 到 `SQLite`（`FileRepo::upsert_batch`，按 `path` 去重）
///   4. `SQLite` 写入失败只记 `warn` 日志，不影响返回给前端的扫描结果（`UI` 优先）
///
/// # Errors
///
/// 路径未通过安全校验或目录读取完全失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn scan_directory(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileInfo>, String> {
    let safe_path = security::validate(&path).map_err(|e| e.to_string())?;
    let files = scan_files_on_disk(&safe_path).map_err(|e| e.to_string())?;

    // —— SQLite 写入：非关键路径，失败只记 warn —— //
    // （扫描是高频 UI 操作，不能因 DB 短暂不可用而阻塞返回）
    match state.db.lock() {
        Ok(conn_guard) => {
            let records: Vec<FileRecord> = files.iter().map(Into::into).collect();
            if let Err(e) = FileRepo::upsert_batch(conn_guard.conn(), &records) {
                log::warn!("scan_directory 写入 SQLite 失败（不影响扫描结果返回）: {e}");
            }
        }
        Err(poisoned) => {
            log::warn!("scan_directory 获取 DB 锁中毒（Mutex poison）: {poisoned}");
        }
    }

    Ok(files)
}

/// 把 IPC 视图 `FileInfo` → 持久化视图 `FileRecord`。
///
/// 约定：`scan_directory` 插入的新记录 `is_deleted=0`；`created_at/updated_at`
/// 由 `SQLite` `datetime('now')` 触发，但 `FileRecord` 字段不允许空，这里
/// 用 `FileInfo` 已有的时间戳占位（Upsert SQL 实际覆盖写入 `datetime('now')`，
/// 所以占位值不会持久化到表中，只是满足字段非空）。
impl From<&FileInfo> for FileRecord {
    fn from(f: &FileInfo) -> Self {
        Self {
            id: f.id.clone(),
            path: f.path.clone(),
            file_name: f.file_name.clone(),
            // u64 → i64：文件大小最大 2^63-1（约 8 EB）足够；超出则饱和到最大值
            file_size: (f.file_size.min(i64::MAX as u64)).cast_signed(),
            content_hash: f.content_hash.clone(),
            category: f.category.clone(),
            is_deleted: false,
            // 占位：Upsert SQL 实际会覆盖为 datetime('now')，参考 file_repo.rs 第 10-18 行
            created_at: f.created_at.clone(),
            updated_at: f.updated_at.clone(),
        }
    }
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

            // content_hash 计算失败（权限/IO）退化为 None，不阻塞整次扫描
            // （避免 1 个不可读文件导致整个扫描目录命令失败）
            let content_hash = compute_file_hash(&path).ok();

            files.push(FileInfo {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_string_lossy().to_string(),
                file_name: entry.file_name().to_string_lossy().to_string(),
                file_size: metadata.len(),
                content_hash,
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
    fn test_scan_files_finds_files_with_hash() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        create_temp_file(tmp.path(), "a.txt", "hello")?;
        create_temp_file(tmp.path(), "b.md", "world")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 2);
        for f in &files {
            let hash = f.content_hash.as_deref().ok_or("hash 未计算")?;
            assert_eq!(hash.len(), 64, "hash 长度应为 64 hex: {hash}");
            assert!(
                hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "hash 非十六进制小写: {hash}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_scan_files_recursive_hash() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let subdir = tmp.path().join("subdir");
        std::fs::create_dir(&subdir)?;
        create_temp_file(tmp.path(), "top.txt", "top")?;
        create_temp_file(&subdir, "nested.txt", "nested")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.file_name == "nested.txt"));
        // 两个不同内容 hash 不同
        let mut hashes: Vec<String> = files
            .iter()
            .map(|f| f.content_hash.clone().expect("hash not none"))
            .collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), 2, "两个不同文件 hash 应该不同");
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

    // ------------------------------------------------------------------
    // scan_directory → SQLite 落库测试（IT-001 的单元层验证）
    // ------------------------------------------------------------------

    #[allow(unused_imports)]
    use super::*;
    use crate::db::Database;
    use crate::sidecar::SidecarManager;
    use crate::AppState;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    /// 构造最小可用 AppState（DB 指向临时 DB，Sidecar/PSK 用占位）。
    fn make_test_app_state(db_path: &std::path::Path) -> AppState {
        let db = Database::open(db_path).expect("打开测试 DB 失败");
        AppState {
            db: std::sync::Mutex::new(db),
            sidecar_manager: Mutex::new(SidecarManager::new(
                "/dev/null/sidecar-nonexistent".into(),
            )),
            sidecar_psk: Mutex::new(None),
            sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
        }
    }

    #[test]
    fn test_scan_directory_writes_sqlite() -> Result<(), Box<dyn std::error::Error>> {
        // 1. 准备：临时扫描目录 + 3 个文件
        let scan_root = tempfile::tempdir()?;
        create_temp_file(scan_root.path(), "one.pdf", "one")?;
        create_temp_file(scan_root.path(), "two.docx", "two")?;
        create_temp_file(scan_root.path(), "three.jpg", "three")?;

        // 2. 准备：临时 SQLite DB + AppState
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 3. 执行：scan_directory（通过包装态调用）
        //    用 tauri State 很难在 test 下构造，这里直接复用内部流程：
        //    先拿 files（等同于扫描），然后走 SQLite upsert 代码段，
        //    保证与 scan_directory 实现的写库逻辑一致
        let files = scan_files_on_disk(scan_root.path())?;
        assert_eq!(files.len(), 3);

        {
            let conn_guard = state.db.lock().unwrap();
            let records: Vec<FileRecord> = files.iter().map(Into::into).collect();
            FileRepo::upsert_batch(conn_guard.conn(), &records)
                .map_err(|e| format!("upsert 失败: {e}"))?;
        }

        // 4. 验证：查 files 表，存在 3 条记录且 hash 非空
        let db = state.db.lock().unwrap();
        let count: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 3, "files 表落库条目数不对");

        let null_hash_count: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND content_hash IS NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(null_hash_count, 0, "有未计算 hash 的文件记录落库");

        Ok(())
    }

    #[test]
    fn test_fileinfo_to_filerecord_from_impl() -> Result<(), Box<dyn std::error::Error>> {
        // From<&FileInfo> for FileRecord 手工验证：is_deleted=false、file_size 转 i64 正确
        let fi = FileInfo {
            id: "id".into(),
            path: "/a/b.txt".into(),
            file_name: "b.txt".into(),
            file_size: 4096,
            content_hash: Some("abc".into()),
            category: Some("cat".into()),
            created_at: "2025-01-01".into(),
            updated_at: "2025-01-02".into(),
        };
        let rec: FileRecord = (&fi).into();
        assert_eq!(rec.id, "id");
        assert_eq!(rec.file_size, 4096);
        assert_eq!(rec.content_hash.as_deref(), Some("abc"));
        assert_eq!(rec.category.as_deref(), Some("cat"));
        assert!(!rec.is_deleted);
        Ok(())
    }
}
