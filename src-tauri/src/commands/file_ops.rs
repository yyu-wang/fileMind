//! 文件操作命令：扫描、预览、批量执行与撤销。
//!
//! 所有命令做阻塞文件系统操作，通过 `#[tauri::command(async)]`
//! 声明为线程池执行，避免阻塞主线程。

use crate::db::models::{FileRecord, OperationLog};
use crate::db::{FileRepo, OperationRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::{self, ConflictStrategy, ConflictType, PlanStatus};
use crate::services::hash_service::compute_file_hash;
use crate::services::operation_executor;
use crate::services::undo_executor;
use crate::{AppState, FileInfo};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::path::{Path, PathBuf};

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
    let mut files = scan_files_on_disk(&safe_path).map_err(|e| e.to_string())?;

    // —— SQLite 写入：非关键路径，失败只记 warn —— //
    // （扫描是高频 UI 操作，不能因 DB 短暂不可用而阻塞返回）
    match state.db.lock() {
        Ok(conn_guard) => {
            if let Err(e) = persist_scan_files(conn_guard.conn(), &mut files) {
                log::warn!("scan_directory 写入 SQLite 失败（不影响扫描结果返回）: {e}");
            }
        }
        Err(poisoned) => {
            log::warn!("scan_directory 获取 DB 锁中毒（Mutex poison）: {poisoned}");
        }
    }

    Ok(files)
}

/// 把扫描结果写入 SQLite，并把 `files` 中的 id 覆盖为入库后的真实 id。
///
/// 扫描生成的 id 是全新 uuid，而同一路径再次扫描时 INSERT 的 `ON CONFLICT(path)`
/// 会保留库内旧 id——若不回写，前端拿到的是库中不存在的 id，后续按 id 反查
/// （`classify_preview` / `preview_operations` / `execute_operations`）会报"文件不存在"。
///
/// 同时回显既存分类：`scan_files_on_disk` 把 category 置为 `None`，但同一路径此前
/// 若被整理过，DB 里已有分类。这里按 path 回查并覆盖回结果，前端才能正确标记
/// 「已整理」、避免重复整理（软排除依赖该字段）。
///
/// # Errors
///
/// 落库失败时返回 `AppError`。
fn persist_scan_files(conn: &rusqlite::Connection, files: &mut [FileInfo]) -> AppResult<()> {
    let records: Vec<FileRecord> = files.iter().map(Into::into).collect();
    let result = FileRepo::upsert_batch(conn, &records)?;
    for f in &mut *files {
        if let Some(real_id) = result.id_map.get(&f.id) {
            f.id.clone_from(real_id);
        }
    }

    // 回显既有分类（DB 中无该路径或未分类的保持 `None`）
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let categories = FileRepo::get_categories_by_paths(conn, &paths)?;
    for f in &mut *files {
        if let Some(category) = categories.get(&f.path) {
            f.category = Some(category.clone());
        }
    }
    Ok(())
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

/// 预览批量文件操作：根据 `file_ids` + `operation` + `target_dir` + `conflict_strategy`
/// 生成 plan，不执行任何文件系统变更。
///
/// 流程（API §s2-2a）：
///   1. 参数校验（`validate_preview_request`）
///   2. 生成 `batch_id`（`uuid4`，供 T3.3 `execute_operations` 接力）
///   3. 从 `SQLite` 反查 `file_ids` 对应的 `FileRecord`（`FileRepo::get_by_ids`）
///   4. 对每个文件调 `build_plan_item` 应用 `conflict_resolver` 生成 `PlanItem`
///   5. 聚合 `summary` 返回
///
/// # Errors
///
/// 参数校验失败返回 `AppError::InvalidInput`；数据库查询失败返回 `AppError::Database`。
#[tauri::command(async)]
#[specta::specta]
pub fn preview_operations(
    request: PreviewRequest,
    state: tauri::State<'_, AppState>,
) -> Result<PreviewResponse, String> {
    let response = preview_operations_inner(request, &state).map_err(|e| e.to_string())?;
    Ok(response)
}

/// 预览逻辑的纯函数入口（便于单元测试，不依赖 `tauri::State`）。
fn preview_operations_inner(
    request: PreviewRequest,
    state: &AppState,
) -> AppResult<PreviewResponse> {
    validate_preview_request(&request)?;

    // 提前生成 batch_id：即使后续失败也能在日志里关联本次预览尝试
    let batch_id = uuid::Uuid::new_v4().to_string();

    // 从 SQLite 反查文件记录（顺序与 file_ids 一致；不存在的 id 静默跳过）
    // MutexGuard<Database> 不 Send+Sync（rusqlite StatementCache 用 RefCell），
    // 用 to_string 把 PoisonError 转为普通字符串错误，避开类型约束
    let files = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::get_by_ids(guard.conn(), &request.file_ids)?
    };

    let strategy = request.conflict_strategy.unwrap_or_default();
    let mut plan = Vec::with_capacity(files.len());
    for f in &files {
        let item = build_plan_item(f, &request, strategy)?;
        plan.push(item);
    }

    let summary = aggregate_summary(&plan);

    Ok(PreviewResponse {
        batch_id,
        plan,
        summary,
    })
}

/// 预览请求参数校验（API §s2-2a 隐含规则）。
///
/// 规则：
///   - `file_ids` 不能为空
///   - `operation = Move | Copy` 时 `target_dir` 必填
///   - `operation = Delete` 时 `target_dir` 可为 `None` 或任意值（忽略不报错）
fn validate_preview_request(req: &PreviewRequest) -> AppResult<()> {
    if req.file_ids.is_empty() {
        return Err(AppError::InvalidInput("file_ids 不能为空".into()));
    }
    match req.operation {
        OperationType::Move | OperationType::Copy | OperationType::Rename => {
            if req.target_dir.as_ref().is_none_or(|s| s.trim().is_empty()) {
                return Err(AppError::InvalidInput(format!(
                    "operation={:?} 需要 target_dir",
                    req.operation
                )));
            }
        }
        OperationType::Delete => {
            // Delete 忽略 target_dir，不校验
        }
    }
    Ok(())
}

/// 把单个 `FileRecord` 转为 `PlanItem`：应用 `conflict_resolver` 算出
/// `new_path` / `status` / `conflict_type`。
///
/// `Delete` 操作直接返回 `new_path=None, status=Ok`，不走 `conflict_resolver`。
fn build_plan_item(
    f: &FileRecord,
    req: &PreviewRequest,
    strategy: ConflictStrategy,
) -> AppResult<PlanItem> {
    let original_path = PathBuf::from(&f.path);

    // Delete 操作：不调 conflict_resolver
    if matches!(req.operation, OperationType::Delete) {
        return Ok(PlanItem {
            file_id: f.id.clone(),
            file_name: f.file_name.clone(),
            original_path: f.path.clone(),
            new_path: None,
            operation: OperationType::Delete,
            status: PlanStatus::Ok,
            conflict_type: None,
        });
    }

    // Move/Copy/Rename：从 target_dir + file_name 拼目标
    let target_dir = req
        .target_dir
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("target_dir 缺失（不应到达此分支）".into()))?;

    // 安全校验目标目录（防止路径遍历到黑名单目录）
    security::validate(target_dir)?;

    let (new_path, status, conflict_type) = conflict_resolver::resolve(
        &f.file_name,
        &original_path,
        Path::new(target_dir),
        strategy,
    );

    Ok(PlanItem {
        file_id: f.id.clone(),
        file_name: f.file_name.clone(),
        original_path: f.path.clone(),
        new_path: new_path.map(|p| p.to_string_lossy().to_string()),
        operation: req.operation.clone(),
        status,
        conflict_type,
    })
}

/// 聚合 plan 生成 summary（按 `PlanStatus` 计数）。
fn aggregate_summary(plan: &[PlanItem]) -> PreviewSummary {
    let total = u32::try_from(plan.len()).unwrap_or(u32::MAX);
    let mut summary = PreviewSummary {
        total,
        ..Default::default()
    };
    for item in plan {
        match item.status {
            PlanStatus::Ok => summary.ok += 1,
            PlanStatus::Conflict => summary.conflict += 1,
            PlanStatus::Error => summary.error += 1,
        }
    }
    summary
}

/// 批量执行文件操作（API §s2-2b）。
///
/// 流程：
///   1. 接收 `ExecuteRequest { batch_id, plan, exclude_file_ids }`
///   2. 预取所有 `file_id → prev_hash`（一次 `FileRepo::get_by_ids`，避免循环 lock）
///   3. 逐项调 `operation_executor::execute_plan_item` 执行
///      - 跳过 `exclude_file_ids` 中的项（计入 `summary.skipped`）
///      - 跳过非 `Ok` 状态的 plan（Conflict/Error，计入 `summary.skipped`）
///   4. 成功后更新 `SQLite` `files` 表：
///      - `Move`/`Rename`：`update_path`（路径变了，文件还在）
///      - `Copy`：源记录不变，新副本留给下次 `scan_directory` 入库
///      - `Delete`：`soft_delete`（标记 `is_deleted=1`，便于 T3.4 undo 恢复）
///   5. 循环结束后一次 `OperationRepo::insert_batch` 写日志（失败只记 warn）
///
/// # Errors
///
/// 仅在严重错误（DB 锁中毒）时返回；单项失败记录在 `results` 中。
#[tauri::command(async)]
#[specta::specta]
pub fn execute_operations(
    request: ExecuteRequest,
    state: tauri::State<'_, AppState>,
) -> Result<ExecuteResponse, String> {
    let response = execute_operations_inner(request, &state).map_err(|e| e.to_string())?;
    Ok(response)
}

/// 执行逻辑纯函数入口（便于单元测试，不依赖 `tauri::State`）。
#[allow(clippy::too_many_lines)]
fn execute_operations_inner(
    request: ExecuteRequest,
    state: &AppState,
) -> AppResult<ExecuteResponse> {
    let exclude_set: HashSet<String> = request.exclude_file_ids.iter().cloned().collect();

    let mut results = Vec::with_capacity(request.plan.len());
    let mut summary = ExecuteSummary {
        total: u32::try_from(request.plan.len()).unwrap_or(u32::MAX),
        ..Default::default()
    };

    // 预取所有 file_id → prev_hash（避免循环里反复 lock）
    let file_ids: Vec<String> = request.plan.iter().map(|p| p.file_id.clone()).collect();
    let prev_hash_map: HashMap<String, Option<String>> = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let records = FileRepo::get_by_ids(guard.conn(), &file_ids)?;
        drop(guard);
        records
            .into_iter()
            .map(|r| (r.id, r.content_hash))
            .collect()
    };

    let mut logs_to_insert: Vec<OperationLog> = Vec::new();

    for item in &request.plan {
        // 跳过用户排除的文件
        if exclude_set.contains(&item.file_id) {
            summary.skipped += 1;
            continue;
        }

        // 「确认执行全部」（resolve_conflicts=true）：冲突项按 Rename 策略重算目标路径
        // （stem_1.ext 递增到不冲突），纳入执行；否则冲突项与 Error 项一律跳过。
        let resolved = if item.status == PlanStatus::Conflict && request.resolve_conflicts {
            item.new_path.as_ref().and_then(|np| {
                Path::new(np).parent().map(|dir| {
                    let (path, status, _) = conflict_resolver::resolve(
                        &item.file_name,
                        Path::new(&item.original_path),
                        dir,
                        ConflictStrategy::Rename,
                    );
                    let mut r = item.clone();
                    r.new_path = path.map(|p| p.to_string_lossy().to_string());
                    r.status = status;
                    r
                })
            })
        } else {
            None
        };

        // 解析后仍非 Ok（非冲突项 / 未要求解析 / 重算异常）→ 跳过
        if item.status != PlanStatus::Ok
            && !matches!(&resolved, Some(r) if r.status == PlanStatus::Ok)
        {
            summary.skipped += 1;
            results.push(ExecuteResult {
                file_id: item.file_id.clone(),
                operation: item.operation.clone(),
                source_path: item.original_path.clone(),
                target_path: item.new_path.clone(),
                success: false,
                error: Some(format!("plan 状态非 Ok: {:?}", item.status)),
                prev_hash: None,
                current_hash: None,
            });
            continue;
        }

        // 实际执行目标：Rename 重算后的项（冲突项）或原项（Ok 项）
        let exec_item = resolved.as_ref().unwrap_or(item);

        let prev_hash = prev_hash_map.get(&exec_item.file_id).cloned().flatten();
        let (success, error, current_hash) =
            operation_executor::execute_plan_item(exec_item, prev_hash.clone());

        if success {
            summary.success += 1;
        } else {
            summary.failed += 1;
        }

        // 构造 operations_log 行（即使失败也写日志，便于审计）
        let log = OperationLog {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: request.batch_id.clone(),
            operation_type: format!("{:?}", exec_item.operation).to_lowercase(),
            source_path: exec_item.original_path.clone(),
            target_path: exec_item.new_path.clone().unwrap_or_default(),
            status: if success { "done" } else { "failed" }.into(),
            prev_hash: prev_hash.clone().unwrap_or_default(),
            current_hash: current_hash.clone().unwrap_or_default(),
            // 链式哈希由 OperationRepo::insert_batch 计算，这里占位空字符串
            chain_hash: String::new(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        logs_to_insert.push(log);

        // 成功后更新 files 表（path / updated_at / 软删除）
        if success {
            if let Ok(guard) = state.db.lock() {
                let conn = guard.conn();
                match exec_item.operation {
                    OperationType::Delete => {
                        if let Err(e) = FileRepo::soft_delete(conn, &exec_item.file_id) {
                            log::warn!(
                                "execute_operations soft_delete 失败（不影响执行结果）: {e}"
                            );
                        }
                    }
                    OperationType::Move | OperationType::Rename => {
                        if let Some(new_path) = &exec_item.new_path {
                            if let Err(e) =
                                FileRepo::update_path(conn, &exec_item.file_id, new_path)
                            {
                                log::warn!(
                                    "execute_operations update_path 失败（不影响执行结果）: {e}"
                                );
                            }
                        }
                    }
                    OperationType::Copy => {
                        // Copy 创建副本，源文件记录不变；新副本不入库（留给后续 scan_directory）
                    }
                }
            }
        }

        results.push(ExecuteResult {
            file_id: exec_item.file_id.clone(),
            operation: exec_item.operation.clone(),
            source_path: exec_item.original_path.clone(),
            target_path: exec_item.new_path.clone(),
            success,
            error,
            prev_hash,
            current_hash,
        });
    }

    // 批量写日志（一次事务）
    if !logs_to_insert.is_empty() {
        if let Ok(guard) = state.db.lock() {
            if let Err(e) = OperationRepo::insert_batch(guard.conn(), &logs_to_insert) {
                log::warn!("execute_operations 写 operations_log 失败（不影响执行结果）: {e}");
            }
        }
    }

    Ok(ExecuteResponse {
        batch_id: request.batch_id,
        results,
        summary,
    })
}

/// 按批次 ID 撤销已执行的批量操作（API §s2-2c）。
///
/// 流程（反向操作链）：
///   1. `OperationRepo::list_by_batch` 拉取该批次日志（ASC 序）
///   2. 校验：批次不存在 / 已撤销 / 含 `delete`（当前不支持）→ 报错
///   3. 反向遍历（DESC），逐条对 `status=done` 的行执行反向文件操作：
///      - `move`/`rename`：`rename(target → source)` + 回写 `files.path`
///      - `copy`：删除 `target_path` 处的副本
///   4. 每项成功后 `update_status(undone)`；单项失败计入 `failed_count`
///
/// # Errors
///
/// 批次不存在、已撤销或含 `delete` 时返回错误；单项撤销失败记录在 `failed_count` 中。
#[tauri::command(async)]
#[specta::specta]
pub fn undo_batch(
    batch_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<UndoResponse, String> {
    undo_batch_inner(&batch_id, &state).map_err(|e| e.to_string())
}

/// 撤销逻辑纯函数入口（便于单元测试，不依赖 `tauri::State`）。
fn undo_batch_inner(batch_id: &str, state: &AppState) -> AppResult<UndoResponse> {
    let logs = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        OperationRepo::list_by_batch(guard.conn(), batch_id)?
    };

    if logs.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "批次不存在或无可撤销项: {batch_id}"
        )));
    }

    // 幂等校验：批次内已有 undone → 拒绝重复撤销
    if logs.iter().any(|l| l.status == "undone") {
        return Err(AppError::Forbidden(format!(
            "批次已撤销，不能重复撤销: {batch_id}"
        )));
    }

    // Delete 为永久删除（T3.3），物理文件无法恢复，暂不支持撤销
    if logs.iter().any(|l| l.operation_type == "delete") {
        return Err(AppError::Forbidden("删除批次暂不支持撤销".into()));
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let mut undone_count = 0u32;
    let mut failed_count = 0u32;

    // 反向遍历：后执行的先撤销（同类操作互相独立，反序仅为语义正确性）
    for log in logs.iter().rev() {
        // 只撤销执行成功的项；failed/pending 无文件副作用可回滚，跳过
        if log.status != "done" {
            continue;
        }

        let undo_result = undo_executor::execute_undo_item(
            &log.operation_type,
            &log.source_path,
            &log.target_path,
        );

        match undo_result {
            Ok(()) => {
                // 物理撤销成功后同步 DB（files.path 回写 + 日志标记 undone）
                if let Ok(guard) = state.db.lock() {
                    let conn = guard.conn();
                    match log.operation_type.as_str() {
                        "move" | "rename" => {
                            if let Ok(Some(rec)) = FileRepo::get_by_path(conn, &log.target_path) {
                                if let Err(e) =
                                    FileRepo::update_path(conn, &rec.id, &log.source_path)
                                {
                                    log::warn!("undo_batch 回写 files.path 失败: {e}");
                                }
                            }
                        }
                        // copy 撤销只删副本，files 表源记录不变
                        _ => {}
                    }
                    if let Err(e) = OperationRepo::update_status(conn, &log.id, "undone") {
                        log::warn!("undo_batch 标记 undone 失败: {e}");
                    }
                }
                undone_count += 1;
            }
            Err(e) => {
                log::warn!("undo_batch 撤销项失败 (id={}): {e}", log.id);
                failed_count += 1;
            }
        }
    }

    Ok(UndoResponse {
        success: failed_count == 0,
        undone_count,
        failed_count,
        task_id,
    })
}

/// 撤销响应体（API §s2-2c 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UndoResponse {
    /// 是否全部撤销成功（无失败项）。
    pub success: bool,
    /// 成功撤销条数。
    pub undone_count: u32,
    /// 撤销失败条数（原路径被占用等）。
    pub failed_count: u32,
    /// 本次撤销任务 ID（uuid，用于关联日志/审计）。
    pub task_id: String,
}

/// 从磁盘根目录扫描文件，收集元信息。
fn scan_files_on_disk(root: &Path) -> AppResult<Vec<FileInfo>> {
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
    ".git",
    ".svn",
    ".hg",
    "dist",
    "build",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".cache",
    ".idea",
    ".vscode",
    "vendor",
];

/// 扫描时跳过的文件（垃圾/临时文件，不区分大小写）。
const SKIP_FILE_NAMES: &[&str] = &[".ds_store", "thumbs.db"];

/// 判断目录名是否命中跳过黑名单。
fn is_skipped_dir(name: &str) -> bool {
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
        log::warn!("超过最大扫描深度 {MAX_SCAN_DEPTH}，跳过: {}", dir.display());
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            // 黑名单目录（node_modules/.git 等）整体跳过，不递归、不入库
            if is_skipped_dir(&name) {
                log::debug!("扫描跳过黑名单目录: {}", path.display());
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

/// 将系统时间格式化为 `YYYY-MM-DD HH:MM:SS`（UTC）。
fn format_system_time(time: std::time::SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 文件操作类型。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum OperationType {
    /// 移动文件（可跨目录，自动创建父目录）。
    Move,
    /// 重命名文件（同目录内改名）。
    Rename,
    /// 复制文件（保留源，目标为新副本）。
    Copy,
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

// ===================================================================
// T3.2 预览接口数据模型（API 规格书 §s2-2a）
// 领域类型 `ConflictStrategy` / `PlanStatus` / `ConflictType` 定义在
// `services::conflict_resolver`，本文件通过 use 引入（保持 services 单向依赖）
// ===================================================================

/// 单个文件的预览计划项（API §s2-2a `plan[]`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PlanItem {
    /// 文件 ID（从 `SQLite` `files.id` 反查得到）。
    pub file_id: String,
    /// 文件名（便于前端展示，不参与路径计算）。
    pub file_name: String,
    /// 源文件绝对路径。
    pub original_path: String,
    /// 目标绝对路径（`Delete` 操作时为 `None`）。
    pub new_path: Option<String>,
    /// 操作类型。
    pub operation: OperationType,
    /// 该项的最终状态。
    pub status: PlanStatus,
    /// 冲突类型（仅在 `status=Conflict` 时有值，其它为 `None`）。
    pub conflict_type: Option<ConflictType>,
}

/// 预览汇总（API §s2-2a `summary`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct PreviewSummary {
    /// 总条目数（等于 `plan.len()`）。
    pub total: u32,
    /// 可执行条目数。
    pub ok: u32,
    /// 冲突条目数。
    pub conflict: u32,
    /// 错误条目数。
    pub error: u32,
}

/// 预览请求体（API §s2-2a）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PreviewRequest {
    /// 待操作的文件 ID 列表（从 `SQLite` 反查路径）。
    pub file_ids: Vec<String>,
    /// 操作类型（`Move`/`Copy`/`Delete`；`Rename` 在预览阶段视为 `Move`）。
    pub operation: OperationType,
    /// 目标目录（`Move`/`Copy` 必填，`Delete` 忽略）。
    pub target_dir: Option<String>,
    /// 冲突策略（`None` 时取默认 `Rename`）。
    pub conflict_strategy: Option<ConflictStrategy>,
}

/// 预览响应体（API §s2-2a 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PreviewResponse {
    /// 本次预览的批次 ID（`uuid4`，供 `execute_operations` 接力使用）。
    pub batch_id: String,
    /// 逐项计划。
    pub plan: Vec<PlanItem>,
    /// 汇总统计。
    pub summary: PreviewSummary,
}

// ===================================================================
// T3.3 执行接口数据模型（API 规格书 §s2-2b）
// ===================================================================

/// 执行请求体（API §s2-2b）。
///
/// 入参 `batch_id` 来自 `preview_operations` 返回值，`plan` 透传该返回值。
/// `exclude_file_ids` 允许用户在预览后取消勾选某些文件，执行时跳过这些 ID。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteRequest {
    /// 来自预览的批次 ID（用于关联 plan 与 `operations_log` 日志）。
    pub batch_id: String,
    /// 透传 `preview_operations` 返回的 plan。
    pub plan: Vec<PlanItem>,
    /// 用户在预览后取消勾选的文件 ID 列表（执行时跳过这些项，计入 summary.skipped）。
    pub exclude_file_ids: Vec<String>,
    /// 是否解析冲突项：`true` 时对 `Conflict` 项按 Rename 策略重算目标路径一并执行
    /// （对应「确认执行全部」）；`false` 时冲突项跳过（对应「仅执行无冲突项」）。
    pub resolve_conflicts: bool,
}

/// 单项执行结果（API §s2-2b `results[]`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteResult {
    /// 文件 ID。
    pub file_id: String,
    /// 操作类型。
    pub operation: OperationType,
    /// 源路径。
    pub source_path: String,
    /// 目标路径（`Delete` 操作时为 `None`）。
    pub target_path: Option<String>,
    /// 是否执行成功。
    pub success: bool,
    /// 失败原因（成功时为 `None`）。
    pub error: Option<String>,
    /// 执行前内容哈希（从 `SQLite` 查出，用于写 `operations_log`）。
    pub prev_hash: Option<String>,
    /// 执行后内容哈希（`Delete` 时为 `None`；`Move`/`Copy` 等于 `prev_hash`）。
    pub current_hash: Option<String>,
}

/// 执行汇总（API §s2-2b `summary`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct ExecuteSummary {
    /// 总条目数（等于 `plan.len()`，含 skipped）。
    pub total: u32,
    /// 成功条数。
    pub success: u32,
    /// 失败条数。
    pub failed: u32,
    /// 跳过条数（`exclude_file_ids` 排除 + 非 `Ok` 状态 plan）。
    pub skipped: u32,
}

/// 执行响应体（API §s2-2b 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ExecuteResponse {
    /// 批次 ID（与请求的 `batch_id` 一致）。
    pub batch_id: String,
    /// 逐项结果。
    pub results: Vec<ExecuteResult>,
    /// 汇总统计。
    pub summary: ExecuteSummary,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]
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
    fn test_scan_skips_blacklist_dirs() -> Result<(), Box<dyn std::error::Error>> {
        // 黑名单目录（node_modules/.git/dist/target/__pycache__/venv）整体跳过
        let tmp = tempfile::tempdir()?;
        for dir in [
            "node_modules",
            ".git",
            "dist",
            "target",
            "__pycache__",
            ".venv",
        ] {
            std::fs::create_dir_all(tmp.path().join(dir))?;
            create_temp_file(&tmp.path().join(dir), "ignored.js", "x")?;
        }
        // 嵌套黑名单：src/node_modules/ 也应跳过
        std::fs::create_dir_all(tmp.path().join("src/node_modules/pkg"))?;
        create_temp_file(&tmp.path().join("src/node_modules/pkg"), "deep.js", "x")?;
        // 正常文件应保留
        create_temp_file(tmp.path(), "keep.txt", "keep")?;
        create_temp_file(&tmp.path().join("src"), "keep2.txt", "keep")?;

        let files = scan_files_on_disk(tmp.path())?;
        let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"keep.txt"));
        assert!(names.contains(&"keep2.txt"));
        assert!(
            !names.contains(&"ignored.js"),
            "黑名单目录内文件不应被扫描: {names:?}"
        );
        assert!(!names.contains(&"deep.js"), "嵌套黑名单目录应跳过");
        Ok(())
    }

    #[test]
    fn test_scan_skips_blacklist_case_insensitive() -> Result<(), Box<dyn std::error::Error>> {
        // 目录名大小写不敏感：Node_Modules 也应跳过
        let tmp = tempfile::tempdir()?;
        std::fs::create_dir_all(tmp.path().join("Node_Modules"))?;
        create_temp_file(&tmp.path().join("Node_Modules"), "a.js", "x")?;
        create_temp_file(tmp.path(), "keep.txt", "keep")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "keep.txt");
        Ok(())
    }

    #[test]
    fn test_scan_skips_garbage_files() -> Result<(), Box<dyn std::error::Error>> {
        // .DS_Store / Thumbs.db 垃圾文件跳过（大小写不敏感）
        let tmp = tempfile::tempdir()?;
        create_temp_file(tmp.path(), ".DS_Store", "")?;
        create_temp_file(tmp.path(), "Thumbs.db", "")?;
        create_temp_file(tmp.path(), "real.txt", "x")?;

        let files = scan_files_on_disk(tmp.path())?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "real.txt");
        Ok(())
    }

    // ------------------------------------------------------------------
    // T3.4 undo_batch 测试
    // ------------------------------------------------------------------

    #[test]
    fn test_undo_batch_move_full_flow() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["a.txt", "b.txt"],
            OperationType::Move,
            Some(target_dir.path().to_string_lossy().to_string()),
            vec![],
        )?;
        assert_eq!(exec.summary.success, 2);

        // 执行后：源目录空，目标目录有文件
        assert!(!scan_root.path().join("a.txt").exists());
        assert!(target_dir.path().join("a.txt").exists());

        let undo = undo_batch_inner(&exec.batch_id, &state)?;
        assert!(undo.success);
        assert_eq!(undo.undone_count, 2);
        assert_eq!(undo.failed_count, 0);

        // 撤销后：文件回到源目录，目标目录空
        assert!(scan_root.path().join("a.txt").exists());
        assert!(scan_root.path().join("b.txt").exists());
        assert!(!target_dir.path().join("a.txt").exists());

        // files 表路径回写 + 日志标记 undone
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let rec = FileRepo::get_by_id(guard.conn(), &exec.results[0].file_id)
            .map_err(|e| e.to_string())?
            .ok_or("files 表应有记录")?;
        assert!(
            rec.path
                .starts_with(scan_root.path().to_string_lossy().as_ref()),
            "撤销后 files.path 应回写源目录: {}",
            rec.path
        );
        let logs = OperationRepo::list_by_batch(guard.conn(), &exec.batch_id)
            .map_err(|e| e.to_string())?;
        assert!(logs.iter().all(|l| l.status == "undone"));
        Ok(())
    }

    #[test]
    fn test_undo_batch_copy_removes_copy() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["orig.txt"],
            OperationType::Copy,
            Some(target_dir.path().to_string_lossy().to_string()),
            vec![],
        )?;
        assert_eq!(exec.summary.success, 1);
        assert!(target_dir.path().join("orig.txt").exists());

        let undo = undo_batch_inner(&exec.batch_id, &state)?;
        assert!(undo.success);
        assert_eq!(undo.undone_count, 1);

        // 副本已删除，源文件保留
        assert!(!target_dir.path().join("orig.txt").exists());
        assert!(scan_root.path().join("orig.txt").exists());
        Ok(())
    }

    #[test]
    fn test_undo_batch_nonexistent_batch_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let result = undo_batch_inner("nonexistent-batch", &state);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::InvalidInput(_)));
        Ok(())
    }

    #[test]
    fn test_undo_batch_delete_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["del.txt"],
            OperationType::Delete,
            None,
            vec![],
        )?;
        assert_eq!(exec.summary.success, 1);

        let result = undo_batch_inner(&exec.batch_id, &state);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Forbidden(_)));
        Ok(())
    }

    #[test]
    fn test_undo_batch_double_undo_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["a.txt"],
            OperationType::Move,
            Some(target_dir.path().to_string_lossy().to_string()),
            vec![],
        )?;

        // 第一次撤销成功
        let undo1 = undo_batch_inner(&exec.batch_id, &state)?;
        assert!(undo1.success);

        // 第二次撤销被拒绝
        let undo2 = undo_batch_inner(&exec.batch_id, &state);
        assert!(undo2.is_err());
        assert!(matches!(undo2.unwrap_err(), AppError::Forbidden(_)));
        Ok(())
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
    fn test_rescan_keeps_stable_ids() -> Result<(), Box<dyn std::error::Error>> {
        // 同一目录重复扫描：返回 id 必须稳定且可被 get_by_ids 反查（修复前新 uuid 未入库）
        let scan_root = tempfile::tempdir()?;
        create_temp_file(scan_root.path(), "a.txt", "aaa")?;
        create_temp_file(scan_root.path(), "b.txt", "bbb")?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let mut first = scan_files_on_disk(scan_root.path())?;
        assert_eq!(first.len(), 2);
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut first)?;
        }

        let mut second = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut second)?;

            // 第二次扫描的临时 id 应回写为第一次入库的 id（同路径同 hash → skip）
            let first_ids: HashSet<&str> = first.iter().map(|f| f.id.as_str()).collect();
            let second_ids: HashSet<&str> = second.iter().map(|f| f.id.as_str()).collect();
            assert_eq!(first_ids, second_ids, "同一目录重复扫描 id 应保持一致");

            // 扫描返回的 id 必须全部可被反查（修复前会落空触发"文件不存在"）
            let ids: Vec<String> = second.iter().map(|f| f.id.clone()).collect();
            let records = FileRepo::get_by_ids(conn_guard.conn(), &ids)?;
            assert_eq!(records.len(), 2, "按扫描返回的 id 反查应全部命中");

            // 库中不应因重复扫描产生重复行
            let count: i64 = conn_guard.conn().query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 2, "重复扫描不应新增记录");
        }
        Ok(())
    }

    #[test]
    fn test_rescan_rehydrates_category() -> Result<(), Box<dyn std::error::Error>> {
        // 已整理文件重新扫描后：返回结果应回显 DB 中既存分类，而非恒为 None
        let scan_root = tempfile::tempdir()?;
        create_temp_file(scan_root.path(), "a.txt", "v1")?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 第一次扫描落库，然后模拟已整理：给文件打上分类标签
        let mut first = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut first)?;
            FileRepo::update_category(conn_guard.conn(), &first[0].id, "文档")?;
        }

        // 重新扫描：内容未变 → upsert 跳过（保留分类），persist 应把分类回显到返回值
        let mut second = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut second)?;
        }

        assert_eq!(
            second[0].category.as_deref(),
            Some("文档"),
            "已整理文件重扫后应回显既存分类"
        );
        Ok(())
    }

    #[test]
    fn test_rescan_unorganized_keeps_none_category() -> Result<(), Box<dyn std::error::Error>> {
        // 未整理过的文件重扫后 category 仍为 None（不回显成别的值）
        let scan_root = tempfile::tempdir()?;
        create_temp_file(scan_root.path(), "raw.txt", "x")?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let mut first = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut first)?;
        }
        let mut second = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut second)?;
        }

        assert!(
            second[0].category.is_none(),
            "未整理文件 category 应保持 None"
        );
        Ok(())
    }

    #[test]
    fn test_upsert_id_map_keeps_id_on_content_change() -> Result<(), Box<dyn std::error::Error>> {
        // 文件内容变化：应 UPDATE 既有行而非新增，且 id 保持不变
        let scan_root = tempfile::tempdir()?;
        create_temp_file(scan_root.path(), "a.txt", "v1")?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let mut first = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut first)?;
        }
        let original_id = first[0].id.clone();

        std::fs::write(scan_root.path().join("a.txt"), b"v2")?;
        let mut second = scan_files_on_disk(scan_root.path())?;
        {
            let conn_guard = state.db.lock().unwrap();
            persist_scan_files(conn_guard.conn(), &mut second)?;
            assert_eq!(second[0].id, original_id, "内容变化时仍应复用原 id");

            let count: i64 = conn_guard.conn().query_row(
                "SELECT COUNT(*) FROM files WHERE path = ?1 AND is_deleted = 0",
                rusqlite::params![second[0].path],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "内容变化应更新而非新增记录");
        }
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

    // ------------------------------------------------------------------
    // T3.2 preview_operations 测试
    // ------------------------------------------------------------------

    /// 往临时 `AppState` 的 `SQLite` 里塞多个文件记录，返回它们的 ID。
    ///
    /// 内部把 `PoisonError<MutexGuard<Database>>` 和 `rusqlite::Error` 转字符串后
    /// 包成 `Box<dyn Error>`，避开 `MutexGuard` 的非 `'static` 借用问题
    /// （`Database` 含 `RefCell`，不满足 `Sync`）。
    fn seed_files_for_preview(
        state: &AppState,
        scan_root: &Path,
        names: &[&str],
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut ids = Vec::new();
        let mut records = Vec::new();
        for name in names {
            let path = scan_root.join(name);
            std::fs::write(&path, b"x")?;
            ids.push(uuid::Uuid::new_v4().to_string());
            records.push(FileRecord {
                id: ids.last().unwrap().clone(),
                path: path.to_string_lossy().to_string(),
                file_name: (*name).to_string(),
                file_size: 1,
                content_hash: Some("dummy".into()),
                category: None,
                is_deleted: false,
                created_at: "2025-01-01".into(),
                updated_at: "2025-01-01".into(),
            });
        }
        let guard = state
            .db
            .lock()
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
        FileRepo::upsert_batch(guard.conn(), &records)
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
        drop(guard);
        Ok(ids)
    }

    #[test]
    fn test_preview_operations_move_rename_strategy() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt", "b.txt", "c.txt"])?;
        assert_eq!(ids.len(), 3);

        let target_dir = tempfile::tempdir()?;
        let req = PreviewRequest {
            file_ids: ids.clone(),
            operation: OperationType::Move,
            target_dir: Some(target_dir.path().to_string_lossy().to_string()),
            conflict_strategy: Some(ConflictStrategy::Rename),
        };

        let resp = preview_operations_inner(req, &state)?;

        // 全部 status=Ok
        assert_eq!(resp.summary.total, 3);
        assert_eq!(resp.summary.ok, 3);
        assert_eq!(resp.summary.conflict, 0);
        assert_eq!(resp.summary.error, 0);

        // new_path = target_dir/{原文件名}
        for item in &resp.plan {
            assert_eq!(item.status, PlanStatus::Ok);
            let new_path = item.new_path.as_ref().expect("Move 应有 new_path");
            assert!(new_path.starts_with(target_dir.path().to_string_lossy().as_ref()));
        }

        // batch_id 是 36 字符 uuid（带连字符，与 uuid::Uuid::new_v4().to_string() 一致）
        assert_eq!(resp.batch_id.len(), 36);
        // 形如 xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
        assert_eq!(resp.batch_id.matches('-').count(), 4);
        assert!(resp
            .batch_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-'));
        Ok(())
    }

    #[test]
    fn test_preview_operations_move_with_conflict_rename() -> Result<(), Box<dyn std::error::Error>>
    {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 源文件
        let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt"])?;

        // 目标目录已存在同名文件 a.txt → 应触发 rename 为 a_1.txt
        let target_dir = tempfile::tempdir()?;
        create_temp_file(target_dir.path(), "a.txt", "existing")?;

        let req = PreviewRequest {
            file_ids: ids,
            operation: OperationType::Move,
            target_dir: Some(target_dir.path().to_string_lossy().to_string()),
            conflict_strategy: Some(ConflictStrategy::Rename),
        };
        let resp = preview_operations_inner(req, &state)?;

        assert_eq!(resp.summary.ok, 1);
        let new_path = resp.plan[0].new_path.as_ref().unwrap();
        assert!(
            new_path.ends_with("a_1.txt"),
            "Rename 策略下冲突应生成 _1 后缀: {new_path}"
        );
        Ok(())
    }

    #[test]
    fn test_preview_operations_delete_no_target_dir() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let ids = seed_files_for_preview(&state, scan_root.path(), &["x.txt", "y.txt"])?;

        let req = PreviewRequest {
            file_ids: ids,
            operation: OperationType::Delete,
            target_dir: None,
            conflict_strategy: None,
        };
        let resp = preview_operations_inner(req, &state)?;

        assert_eq!(resp.summary.total, 2);
        assert_eq!(resp.summary.ok, 2);
        for item in &resp.plan {
            assert!(item.new_path.is_none(), "Delete 操作 new_path 应为 None");
            assert_eq!(item.operation, OperationType::Delete);
        }
        Ok(())
    }

    #[test]
    fn test_preview_operations_missing_target_dir_for_move() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let state = make_test_app_state(tmp_db.path());

        let req = PreviewRequest {
            file_ids: vec!["fake-id".into()],
            operation: OperationType::Move,
            target_dir: None,
            conflict_strategy: None,
        };
        let result = preview_operations_inner(req, &state);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, AppError::InvalidInput(_)),
            "期望 InvalidInput"
        );
    }

    #[test]
    fn test_preview_operations_empty_file_ids() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let state = make_test_app_state(tmp_db.path());

        let req = PreviewRequest {
            file_ids: vec![],
            operation: OperationType::Delete,
            target_dir: None,
            conflict_strategy: None,
        };
        let result = preview_operations_inner(req, &state);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::InvalidInput(_)));
    }

    #[test]
    fn test_preview_operations_unknown_file_id_silently_skipped(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let real_ids = seed_files_for_preview(&state, scan_root.path(), &["real.txt"])?;
        let mut all_ids = real_ids.clone();
        all_ids.push("non-existent-uuid".into()); // 不存在的 ID

        let target_dir = tempfile::tempdir()?;
        let req = PreviewRequest {
            file_ids: all_ids,
            operation: OperationType::Move,
            target_dir: Some(target_dir.path().to_string_lossy().to_string()),
            conflict_strategy: None, // 默认 Rename
        };

        let resp = preview_operations_inner(req, &state)?;
        // 不存在的 ID 静默跳过 → plan 只有 1 项（真实文件）
        assert_eq!(resp.plan.len(), 1);
        assert_eq!(resp.summary.total, 1);
        assert_eq!(resp.summary.ok, 1);
        Ok(())
    }

    #[test]
    fn test_preview_operations_batch_id_is_uuid_format() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let ids = seed_files_for_preview(&state, scan_root.path(), &["only.txt"])?;

        let req = PreviewRequest {
            file_ids: ids,
            operation: OperationType::Delete,
            target_dir: None,
            conflict_strategy: None,
        };
        let resp = preview_operations_inner(req, &state)?;

        // uuid4 → 36 字符（带连字符）
        assert_eq!(resp.batch_id.len(), 36);
        assert_eq!(resp.batch_id.matches('-').count(), 4);
        assert!(resp
            .batch_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-'));
        Ok(())
    }

    // ------------------------------------------------------------------
    // T3.3 execute_operations 测试
    // ------------------------------------------------------------------

    /// 辅助：从 preview 拿 plan 后调 execute，返回 `ExecuteResponse`。
    fn preview_then_execute(
        state: &AppState,
        scan_root: &Path,
        names: &[&str],
        operation: OperationType,
        target_dir: Option<String>,
        exclude: Vec<String>,
    ) -> Result<(PreviewResponse, ExecuteResponse), Box<dyn std::error::Error>> {
        let ids = seed_files_for_preview(state, scan_root, names)?;
        let preview = preview_operations_inner(
            PreviewRequest {
                file_ids: ids.clone(),
                operation,
                target_dir,
                conflict_strategy: Some(ConflictStrategy::Rename),
            },
            state,
        )?;
        let execute = execute_operations_inner(
            ExecuteRequest {
                batch_id: preview.batch_id.clone(),
                plan: preview.plan.clone(),
                exclude_file_ids: exclude,
                resolve_conflicts: false,
            },
            state,
        )?;
        Ok((preview, execute))
    }

    #[test]
    fn test_execute_operations_move_full_flow() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["a.txt", "b.txt", "c.txt"],
            OperationType::Move,
            Some(target_dir.path().to_string_lossy().to_string()),
            vec![],
        )?;

        // 全部成功
        assert_eq!(exec.summary.total, 3);
        assert_eq!(exec.summary.success, 3);
        assert_eq!(exec.summary.failed, 0);
        assert_eq!(exec.summary.skipped, 0);

        // SQLite files 表路径已更新
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        for r in &exec.results {
            let file = FileRepo::get_by_id(guard.conn(), &r.file_id).map_err(|e| e.to_string())?;
            assert!(file.is_some(), "files 表应有记录");
            let f = file.unwrap();
            assert!(
                f.path
                    .starts_with(target_dir.path().to_string_lossy().as_ref()),
                "files.path 应已更新到目标目录: {}",
                f.path
            );
            assert!(!f.is_deleted, "Move 后不应软删除");
        }

        // operations_log 有 3 条 done 记录
        let logs = OperationRepo::list_by_batch(guard.conn(), &exec.batch_id)
            .map_err(|e| e.to_string())?;
        assert_eq!(logs.len(), 3);
        assert!(logs.iter().all(|l| l.status == "done"));
        assert!(logs.iter().all(|l| l.batch_id == exec.batch_id));
        Ok(())
    }

    #[test]
    fn test_execute_operations_with_exclude() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 先 preview 拿到 3 个 plan
        let ids = seed_files_for_preview(&state, scan_root.path(), &["a.txt", "b.txt", "c.txt"])?;
        let preview = preview_operations_inner(
            PreviewRequest {
                file_ids: ids.clone(),
                operation: OperationType::Move,
                target_dir: Some(target_dir.path().to_string_lossy().to_string()),
                conflict_strategy: Some(ConflictStrategy::Rename),
            },
            &state,
        )?;

        // 排除第 2 个文件
        let excluded_id = ids[1].clone();
        let exec = execute_operations_inner(
            ExecuteRequest {
                batch_id: preview.batch_id.clone(),
                plan: preview.plan.clone(),
                exclude_file_ids: vec![excluded_id.clone()],
                resolve_conflicts: false,
            },
            &state,
        )?;

        assert_eq!(exec.summary.total, 3);
        assert_eq!(exec.summary.success, 2);
        assert_eq!(exec.summary.skipped, 1);
        assert_eq!(exec.summary.failed, 0);

        // 排除项不应出现在 results 里
        assert!(
            !exec.results.iter().any(|r| r.file_id == excluded_id),
            "排除项不应在 results 中"
        );

        // operations_log 应该只有 2 条（排除项没写日志）
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let logs = OperationRepo::list_by_batch(guard.conn(), &exec.batch_id)
            .map_err(|e| e.to_string())?;
        assert_eq!(logs.len(), 2);
        Ok(())
    }

    #[test]
    fn test_execute_operations_delete_soft_deletes() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["del1.txt", "del2.txt"],
            OperationType::Delete,
            None,
            vec![],
        )?;

        assert_eq!(exec.summary.success, 2);

        // files 表记录应被软删除
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        for r in &exec.results {
            // get_by_id 只返回未删除记录，软删除后应返回 None
            let file = FileRepo::get_by_id(guard.conn(), &r.file_id).map_err(|e| e.to_string())?;
            assert!(file.is_none(), "Delete 后记录应被软删除");
        }

        // operations_log 有 2 条 done 记录，operation_type='delete'
        let logs = OperationRepo::list_by_batch(guard.conn(), &exec.batch_id)
            .map_err(|e| e.to_string())?;
        assert_eq!(logs.len(), 2);
        assert!(logs.iter().all(|l| l.operation_type == "delete"));
        assert!(logs.iter().all(|l| l.target_path.is_empty()));
        Ok(())
    }

    #[test]
    fn test_execute_operations_copy_does_not_modify_files_table(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, exec) = preview_then_execute(
            &state,
            scan_root.path(),
            &["orig.txt"],
            OperationType::Copy,
            Some(target_dir.path().to_string_lossy().to_string()),
            vec![],
        )?;

        assert_eq!(exec.summary.success, 1);

        // Copy 后 files 表原记录路径不变
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let file = FileRepo::get_by_id(guard.conn(), &exec.results[0].file_id)
            .map_err(|e| e.to_string())?;
        let f = file.expect("files 表应有原记录");
        assert!(
            f.path
                .starts_with(scan_root.path().to_string_lossy().as_ref()),
            "Copy 后源记录路径不应变化"
        );
        assert!(!f.is_deleted);
        Ok(())
    }

    #[test]
    fn test_execute_operations_failed_item_still_writes_log(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?; // 真实存在的目录，让 preview 通过
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 先 preview 拿到 plan
        let ids = seed_files_for_preview(&state, scan_root.path(), &["real.txt"])?;
        let preview = preview_operations_inner(
            PreviewRequest {
                file_ids: ids,
                operation: OperationType::Move,
                target_dir: Some(target_dir.path().to_string_lossy().to_string()),
                conflict_strategy: Some(ConflictStrategy::Rename),
            },
            &state,
        )?;

        // 把源文件删除，让 execute 阶段失败（preview 已通过）
        std::fs::remove_file(scan_root.path().join("real.txt"))?;

        let exec = execute_operations_inner(
            ExecuteRequest {
                batch_id: preview.batch_id.clone(),
                plan: preview.plan.clone(),
                exclude_file_ids: vec![],
                resolve_conflicts: false,
            },
            &state,
        )?;

        // 失败也算入 total，但不计入 skipped
        assert_eq!(exec.summary.total, 1);
        assert_eq!(exec.summary.success, 0);
        assert_eq!(exec.summary.failed, 1);

        // 失败的项也写日志，status=failed
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let logs = OperationRepo::list_by_batch(guard.conn(), &exec.batch_id)
            .map_err(|e| e.to_string())?;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "failed");
        Ok(())
    }

    // ------------------------------------------------------------------
    // T6.x 冲突项 Rename 解析（确认执行全部）
    // ------------------------------------------------------------------

    /// 构造冲突项 plan：目标目录已有同名文件 → status=Conflict。
    fn make_conflict_plan(
        state: &AppState,
        scan_root: &Path,
        target_dir: &Path,
        name: &str,
    ) -> Result<(String, Vec<PlanItem>), Box<dyn std::error::Error>> {
        // 目标目录已存在同名文件 → 构造 Conflict 项
        std::fs::write(target_dir.join(name), b"existing")?;
        std::fs::write(scan_root.join(name), b"src")?;
        let ids = seed_files_for_preview(state, scan_root, &[name])?;
        let plan = vec![PlanItem {
            file_id: ids[0].clone(),
            file_name: name.to_string(),
            original_path: scan_root.join(name).to_string_lossy().to_string(),
            new_path: Some(target_dir.join(name).to_string_lossy().to_string()),
            operation: OperationType::Move,
            status: PlanStatus::Conflict,
            conflict_type: Some(ConflictType::SameName),
        }];
        Ok((ids[0].clone(), plan))
    }

    #[test]
    fn test_execute_resolve_conflicts_renames_target() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (file_id, plan) =
            make_conflict_plan(&state, scan_root.path(), target_dir.path(), "a.txt")?;
        let exec = execute_operations_inner(
            ExecuteRequest {
                batch_id: "b-resolve".into(),
                plan,
                exclude_file_ids: vec![],
                resolve_conflicts: true,
            },
            &state,
        )?;

        // 冲突项经 Rename 解析后执行成功：生成 a_1.txt，原同名文件保留
        assert_eq!(exec.summary.total, 1);
        assert_eq!(exec.summary.success, 1);
        assert_eq!(exec.summary.skipped, 0);
        assert!(target_dir.path().join("a_1.txt").exists(), "应生成 a_1.txt");
        assert!(target_dir.path().join("a.txt").exists(), "原同名文件应保留");
        assert!(!scan_root.path().join("a.txt").exists(), "源文件应已移走");

        // files 表路径回写到 a_1.txt
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let rec = FileRepo::get_by_id(guard.conn(), &file_id)
            .map_err(|e| e.to_string())?
            .ok_or("文件应存在")?;
        assert!(
            rec.path.ends_with("a_1.txt"),
            "files.path 应回写为 a_1.txt: {}",
            rec.path
        );
        Ok(())
    }

    #[test]
    fn test_execute_skips_conflicts_when_not_resolving() -> Result<(), Box<dyn std::error::Error>> {
        let scan_root = tempfile::tempdir()?;
        let target_dir = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let (_, plan) = make_conflict_plan(&state, scan_root.path(), target_dir.path(), "a.txt")?;
        let exec = execute_operations_inner(
            ExecuteRequest {
                batch_id: "b-skip".into(),
                plan,
                exclude_file_ids: vec![],
                resolve_conflicts: false,
            },
            &state,
        )?;

        // 不解析冲突：跳过，源文件不动
        assert_eq!(exec.summary.total, 1);
        assert_eq!(exec.summary.success, 0);
        assert_eq!(exec.summary.skipped, 1);
        assert!(scan_root.path().join("a.txt").exists());
        assert!(!target_dir.path().join("a_1.txt").exists());
        Ok(())
    }
}
