//! 扫描与落库：目录扫描命令、增量落库、`FileInfo` → `FileRecord` 视图转换。
//!
//! 增量策略（T10.1）：短锁取快照 → 锁外并行重算变化文件的 hash → 短锁批量 upsert，
//! 避免长锁阻塞事件循环。

use crate::db::models::FileRecord;
use crate::db::{FileRepo, ScannedDirectoryRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::hash_service::compute_hashes_parallel;
use crate::{AppState, FileInfo};
use std::convert::From;
use std::path::Path;

// 同层子模块：拆分前同文件内直接可见，拆后需显式引入
use super::fs_walk::scan_files_on_disk;

/// 递归扫描目录并返回文件元信息列表（含 `content_hash`），并 `upsert` 到 `SQLite`。
///
/// 行为（T10.1 增量扫描重构后）：
///   1. 路径安全校验（`security::validate`）
///   2. 递归扫描目录 + 读取元信息（**不含 hash**，hash 计算移出锁外并行进行）
///   3. `persist_scan_files`：短锁批量读快照 → 释放锁 → 锁外标记未变化文件并并行重算
///      变化文件的 SHA-256（`compute_hashes_parallel`）→ 短锁批量 `upsert`
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
    let mut files = scan_files_on_disk(&safe_path).map_err(|e| e.to_string())?;

    // —— SQLite 写入：非关键路径，失败只记 warn —— //
    // （扫描是高频 UI 操作，不能因 DB 短暂不可用而阻塞返回）
    match persist_scan_files(&state.db, &mut files) {
        Ok(()) => {}
        Err(e) => log::warn!("scan_directory 写入 SQLite 失败（不影响扫描结果返回）: {e}"),
    }

    // —— 记录已扫描目录（非关键路径，失败只记 warn）—— //
    // 目录级移除功能依赖该表；同一路径重复扫描只更新 updated_at
    if let Ok(guard) = state.db.lock() {
        if let Err(e) = ScannedDirectoryRepo::insert_or_update(guard.conn(), &path) {
            log::warn!("scan_directory 记录扫描目录失败（不影响扫描结果返回）: {e}");
        }
    } else {
        log::warn!("scan_directory 获取 DB 锁失败（跳过目录记录）");
    }

    Ok(files)
}

/// 把扫描结果写入 SQLite，并把 `files` 中的 id 覆盖为入库后的真实 id。
///
/// 参数取 `&Mutex<Database>` 而非 `&AppState`：命令层拆锁传入，基准/集成测试
/// （T10.5 `scan_perf.rs`）可直接构造临时库调用，无需依赖 `tauri::State`。
///
/// 阶段化（短锁 + 锁外并行 hash，T10.1 增量扫描核心）：
///   1. **短锁**：`load_snapshots_by_paths` 一条批量 `IN` 查询加载全部既有快照
///   2. **锁外**：`(size, mtime)` 与快照一致 → 复用 `content_hash` 跳过重算；
///      仅对变化文件 `compute_hashes_parallel` 并行重算
///   3. **短锁**：`upsert_batch_with_snapshots` 批量落库（跳过/更新/插入一次完成）
///
/// 回写说明：扫描生成的 id 是全新 uuid，而同一路径再次扫描时 INSERT 的
/// `ON CONFLICT(path)` 会保留库内旧 id——若不回写，前端拿到的是库中不存在的 id，
/// 后续按 id 反查（`classify_preview` / `preview_operations` / `execute_operations`）
/// 会报"文件不存在"。
///
/// 同时回显既存分类：`scan_files_on_disk` 把 category 置为 `None`，但同一路径此前
/// 若被整理过，DB 里已有分类。这里按 path 回查并覆盖回结果，前端才能正确标记
/// 「已整理」、避免重复整理（软排除依赖该字段）。
///
/// # Errors
///
/// 落库失败时返回 `AppError`。
pub(super) fn persist_scan_files(
    db: &std::sync::Mutex<crate::db::Database>,
    files: &mut [FileInfo],
) -> AppResult<()> {
    // 阶段 1：短锁批量读快照（锁在该作用域结束自动释放）
    let snapshots = {
        let guard = db.lock().map_err(|poisoned| {
            AppError::Internal(format!(
                "scan_directory 获取 DB 锁中毒（Mutex poison）: {poisoned}"
            ))
        })?;
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        FileRepo::load_snapshots_by_paths(guard.conn(), &paths)?
    };

    // 阶段 2：锁外 —— 未变化文件复用既有 hash，仅变化文件并行重算
    let needs_hash: Vec<bool> = files
        .iter_mut()
        .map(|f| {
            // 磁盘 (size, mtime) 与快照一致且既有 hash 可用 → 复用，跳过重算
            let reusable = match snapshots.get(&f.path) {
                Some(s)
                    if !s.is_deleted
                        && s.content_hash.is_some()
                        && s.file_size == f.file_size.cast_signed()
                        && s.mtime.as_deref() == Some(f.updated_at.as_str()) =>
                {
                    f.content_hash = s.content_hash.clone();
                    true
                }
                _ => false,
            };
            !reusable
        })
        .collect();
    compute_hashes_parallel(files, &needs_hash);

    // 阶段 3：短锁批量落库 + 回写 id + 回显分类
    let guard = db.lock().map_err(|poisoned| {
        AppError::Internal(format!(
            "scan_directory 获取 DB 锁中毒（Mutex poison）: {poisoned}"
        ))
    })?;
    let records: Vec<FileRecord> = files.iter().map(Into::into).collect();
    let result = FileRepo::upsert_batch_with_snapshots(guard.conn(), &records, &snapshots)?;
    for f in &mut *files {
        if let Some(real_id) = result.id_map.get(&f.id) {
            f.id.clone_from(real_id);
        }
    }

    // 回显既有分类（DB 中无该路径或未分类的保持 `None`）
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let categories = FileRepo::get_categories_by_paths(guard.conn(), &paths)?;
    for f in &mut *files {
        if let Some(category) = categories.get(&f.path) {
            f.category = Some(category.clone());
        }
    }
    // 阶段 3 的锁用到最后一次查询即释放，避免长持锁（锁外后续无 DB 操作）
    drop(guard);
    Ok(())
}

/// T10.5：扫描 + 增量落库全流程（基准 / 集成测试入口）。
///
/// 与 `scan_directory` 命令同路径（`scan_files_on_disk` → `persist_scan_files`），
/// 但去掉 `tauri::State` 与路径安全校验依赖，供 `src-tauri/tests/scan_perf.rs`
/// 直接构造临时库 + 临时目录度量全量 / 增量扫描耗时。
///
/// # Errors
///
/// 扫描或落库失败时返回 `AppError`。
#[doc(hidden)]
pub fn scan_and_persist(
    db: &std::sync::Mutex<crate::db::Database>,
    root: &Path,
) -> AppResult<Vec<FileInfo>> {
    let mut files = scan_files_on_disk(root)?;
    persist_scan_files(db, &mut files)?;
    Ok(files)
}

/// 把 IPC 视图 `FileInfo` → 持久化视图 `FileRecord`。
///
/// 约定：`scan_directory` 插入的新记录 `is_deleted=0`；`created_at/updated_at`
/// 由 `SQLite` `datetime('now')` 触发，但 `FileRecord` 字段不允许空，这里
/// 用 `FileInfo` 已有的时间戳占位（Upsert SQL 实际覆盖写入 `datetime('now')`，
/// 所以占位值不会持久化到表中，只是满足字段非空）。
///
/// `mtime` 取自 `FileInfo.updated_at`（T10.1：扫描阶段该字段就是磁盘真实 mtime，
/// 尚未被 `datetime('now')` 覆盖），用于增量跳过判定。
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
            mtime: Some(f.updated_at.clone()),
        }
    }
}
