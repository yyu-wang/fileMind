//! 磁盘遍历底层：递归扫描、系统/隐藏目录与文件黑名单、时间格式化。

use crate::error::AppResult;
use crate::security;
use crate::FileInfo;
use chrono::{DateTime, Utc};
use std::path::Path;

pub(super) const MAX_SCAN_DEPTH: u32 = 10;
/// 从磁盘根目录扫描文件，收集元信息。
pub(super) fn scan_files_on_disk(root: &Path) -> AppResult<Vec<FileInfo>> {
    let mut files = Vec::new();
    scan_dir_recursive(root, &mut files, 0)?;
    Ok(files)
}

/// 扫描时跳过的目录名（任意层级命中即跳过，不区分大小写）。
///
/// 主要针对工程代码库的依赖/构建/版本控制目录：`node_modules` 等动辄数十万文件，
/// 入库会拖垮文件库规模与 UI 性能（见历史问题：63.9 万文件卡死）。
const SKIP_DIR_NAMES: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "target",
    "__pycache__",
    "venv",
    "vendor",
    // Windows 系统目录
    "$recycle.bin",
    "system volume information",
    // macOS 系统目录
    "__macosx",
    ".spotlight-v100",
    ".fseventsd",
    ".trashes",
    // 通用缓存/日志目录
    "cache",
    "caches",
    "logs",
];

/// 扫描时跳过的文件（垃圾/临时文件，不区分大小写）。
const SKIP_FILE_NAMES: &[&str] = &[".ds_store", "thumbs.db"];

/// 判断目录名是否命中跳过黑名单。
///
/// 跳过优先级：
/// 1. 隐藏目录（以 `.` 开头）——统一跳过，系统/应用数据，非用户文件
/// 2. 显式黑名单目录（`SKIP_DIR_NAMES`）——工程依赖、系统目录
fn is_skipped_dir(name: &str) -> bool {
    // 隐藏目录兜底：以 `.` 开头的目录在 Finder 中默认不可见，
    // 绝大多数是系统/应用数据，不应纳入文件整理范围
    if name.starts_with('.') {
        return true;
    }
    let lower = name.to_lowercase();
    SKIP_DIR_NAMES.contains(&lower.as_str())
}

/// 判断文件名是否命中跳过黑名单。
fn is_skipped_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    SKIP_FILE_NAMES.contains(&lower.as_str())
}

/// 递归遍历目录，超过最大深度时跳过并记录警告。
fn scan_dir_recursive(dir: &Path, files: &mut Vec<FileInfo>, depth: u32) -> AppResult<()> {
    if depth > MAX_SCAN_DEPTH {
        log::warn!(
            "超过最大扫描深度 {MAX_SCAN_DEPTH}，跳过: {}",
            security::log_redact::sanitize_path(&dir.display().to_string())
        );
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            // 黑名单目录（node_modules/.git 等）整体跳过，不递归、不入库
            if is_skipped_dir(&name) {
                log::debug!(
                    "扫描跳过黑名单目录: {}",
                    security::log_redact::sanitize_path(&path.display().to_string())
                );
                continue;
            }
            scan_dir_recursive(&path, files, depth + 1)?;
        } else if path.is_file() {
            // 垃圾/临时文件（.DS_Store 等）跳过
            if is_skipped_file(&name) {
                continue;
            }
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

            // hash 不在收集阶段计算（T10.1）：collection 只收元信息，
            // 之后在锁外对「变化文件」并行重算（persist_scan_files 阶段 2）。
            // 单文件 hash 失败同样退化为 None，不阻塞整次扫描。
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

/// 将系统时间格式化为 `YYYY-MM-DD HH:MM:SS`（UTC）。
pub(super) fn format_system_time(time: std::time::SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}
