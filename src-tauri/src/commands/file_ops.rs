//! 文件操作命令：扫描、预览、批量执行与撤销。
//!
//! 所有命令做阻塞文件系统操作，通过 `#[tauri::command(async)]`
//! 声明为线程池执行，避免阻塞主线程。

use crate::db::models::{FileRecord, OperationLog};
use crate::db::{ConfigRepo, FileRepo, OperationRepo, ScannedDirectoryRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::{self, ConflictStrategy, ConflictType, PlanStatus};
use crate::services::hash_service::compute_hashes_parallel;
use crate::services::operation_executor;
use crate::services::undo_executor;
use crate::sidecar::proxy;
use crate::{AppState, FileInfo};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::path::{Path, PathBuf};

const MAX_SCAN_DEPTH: u32 = 10;

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
fn persist_scan_files(
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

/// `/index/update_paths` 请求体（对齐 sidecar `IndexPathUpdateRequest`）。
#[derive(Debug, Serialize)]
struct SidecarPathUpdateItem {
    file_id: String,
    path: String,
}

/// `/index/update_paths` 请求体。
#[derive(Debug, Serialize)]
struct SidecarPathUpdateRequest {
    table_name: String,
    mappings: Vec<SidecarPathUpdateItem>,
}

/// 尽力而为：把分类移动/撤销后的文件路径同步到向量索引。
///
/// `SQLite` 的 `files.path` 已是新路径，但 `LanceDB` 向量行里的 `file_path` 仍是旧路径
/// （索引增量逻辑只认 created/modified/deleted，无移动语义），RAG 问答引用会指向
/// 失效位置。这里经 `/index/update_paths` 原地更新 `file_path`（向量不变，不重新
/// embedding）。纯 best-effort：sidecar 未就绪或失败仅记录日志，绝不影响执行结果。
fn spawn_index_path_sync(state: &AppState, mappings: Vec<(String, String)>) {
    if mappings.is_empty() {
        return;
    }

    // 目标表名由当前 embedding 模型决定（对齐 build_index 的 documents_{model}_v1）
    let table_name = match state.db.lock() {
        Ok(guard) => match ConfigRepo::get(guard.conn()) {
            Ok(config) => format!("documents_{}_v1", config.embedding_model),
            Err(e) => {
                log::warn!("索引路径同步：读取 embedding 模型失败（跳过）: {e}");
                return;
            }
        },
        Err(poisoned) => {
            log::warn!("索引路径同步：获取 DB 锁中毒（跳过）: {poisoned}");
            return;
        }
    };

    // sidecar 未握手（PSK 为空，如测试环境）→ 静默跳过，不 spawn
    let psk = match state.sidecar_psk.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("索引路径同步：获取 PSK 锁中毒（跳过）: {poisoned}");
            return;
        }
    };
    let Some(psk) = psk else {
        return;
    };
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let count = mappings.len();
    let mappings: Vec<SidecarPathUpdateItem> = mappings
        .into_iter()
        .map(|(file_id, path)| SidecarPathUpdateItem { file_id, path })
        .collect();

    tauri::async_runtime::spawn(async move {
        let request = SidecarPathUpdateRequest {
            table_name,
            mappings,
        };
        let body = match serde_json::to_string(&request) {
            Ok(body) => body,
            Err(e) => {
                log::warn!("索引路径同步：序列化请求失败（跳过）: {e}");
                return;
            }
        };
        match proxy::forward_post("/index/update_paths", &body, &psk, seq).await {
            Ok(_) => log::info!("索引路径已同步 {count} 个文件"),
            Err(e) => log::warn!("索引路径同步失败（不影响执行结果）: {e}"),
        }
    });
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
    // 路径被移动/重命名的文件（file_id, 新路径），执行后同步到向量索引
    let mut moved_paths: Vec<(String, String)> = Vec::new();

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
                            } else {
                                moved_paths.push((exec_item.file_id.clone(), new_path.clone()));
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

    // 尽力而为：把移动后文件的最新路径同步到向量索引（失败仅告警）
    spawn_index_path_sync(state, moved_paths);

    Ok(ExecuteResponse {
        batch_id: request.batch_id,
        results,
        summary,
    })
}

/// 单个文件删除结果（移入系统回收站）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DeleteFilesResult {
    /// 文件 id。
    pub file_id: String,
    /// 是否成功移入系统回收站。
    pub success: bool,
    /// 失败原因（成功时为 `None`）。
    pub error: Option<String>,
}

/// 删除失败结果的简构（避免重复三元组）。
fn delete_files_failure(file_id: String, reason: &str) -> DeleteFilesResult {
    DeleteFilesResult {
        file_id,
        success: false,
        error: Some(reason.to_string()),
    }
}

/// 删除文件：把选中的文件移入系统回收站（安全网，非物理删除）。
///
/// 逐个执行（单个失败不阻断其余文件）：
///   1. 路径安全校验 + 磁盘文件存在性检查
///   2. 移入系统回收站（`operation_executor` 的 `Delete` 语义）
///   3. 成功后 `files` 软删除（`is_deleted=1`）并写入 `operations_log` 审计行
///
/// 与分类预览的 `Delete` 计划共用同一执行器，保证删除语义唯一。删除批次
/// 不参与应用内撤销（文件可从系统回收站手动恢复）。
///
/// # Errors
///
/// 入参全为不可删除（空/全部不存在）时返回成功空列表；单文件失败以
/// `DeleteFilesResult.success=false` 返回，不中断整批。
#[tauri::command(async)]
#[specta::specta]
pub fn delete_files(
    file_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DeleteFilesResult>, String> {
    delete_files_inner(&file_ids, &state).map_err(|e| e.to_string())
}

/// 删除纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
fn delete_files_inner(file_ids: &[String], state: &AppState) -> AppResult<Vec<DeleteFilesResult>> {
    // 去重（保序）：同一文件重复提交只删一次
    let mut seen = HashSet::with_capacity(file_ids.len());
    let mut ids: Vec<String> = Vec::with_capacity(file_ids.len());
    for file_id in file_ids {
        if seen.insert(file_id) {
            ids.push(file_id.clone());
        }
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let records = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::get_by_ids(guard.conn(), &ids)?
    };
    let record_map: HashMap<String, FileRecord> =
        records.into_iter().map(|r| (r.id.clone(), r)).collect();

    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut results = Vec::with_capacity(ids.len());
    let mut logs_to_insert: Vec<OperationLog> = Vec::new();

    for file_id in ids {
        let Some(record) = record_map.get(&file_id) else {
            results.push(delete_files_failure(file_id, "文件不存在或已删除"));
            continue;
        };
        if record.is_deleted {
            results.push(delete_files_failure(file_id, "文件已删除"));
            continue;
        }
        let source = match security::validate(&record.path) {
            Ok(path) => path,
            Err(e) => {
                results.push(delete_files_failure(file_id, &e.to_string()));
                continue;
            }
        };
        if !source.is_file() {
            results.push(delete_files_failure(
                file_id,
                "文件不存在（可能已被外部移除）",
            ));
            continue;
        }

        let item = PlanItem {
            file_id: file_id.clone(),
            file_name: record.file_name.clone(),
            original_path: record.path.clone(),
            new_path: None,
            operation: OperationType::Delete,
            status: PlanStatus::Ok,
            conflict_type: None,
        };
        let (success, error, current_hash) =
            operation_executor::execute_plan_item(&item, record.content_hash.clone());

        if success {
            // 软删除：磁盘已进回收站，DB 标记 is_deleted=1（失败仅告警）
            if let Ok(guard) = state.db.lock() {
                if let Err(e) = FileRepo::soft_delete(guard.conn(), &file_id) {
                    log::warn!("delete_files soft_delete 失败（不影响删除结果）: {e}");
                }
            }
        }

        logs_to_insert.push(OperationLog {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: batch_id.clone(),
            operation_type: "delete".to_string(),
            source_path: record.path.clone(),
            target_path: String::new(),
            status: if success { "done" } else { "failed" }.into(),
            prev_hash: record.content_hash.clone().unwrap_or_default(),
            current_hash: current_hash.unwrap_or_default(),
            chain_hash: String::new(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });
        results.push(DeleteFilesResult {
            file_id,
            success,
            error,
        });
    }

    // 批量写审计日志（链式哈希由 OperationRepo::insert_batch 计算）
    if !logs_to_insert.is_empty() {
        if let Ok(guard) = state.db.lock() {
            if let Err(e) = OperationRepo::insert_batch(guard.conn(), &logs_to_insert) {
                log::warn!("delete_files 写 operations_log 失败（不影响删除结果）: {e}");
            }
        }
    }

    Ok(results)
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

    // 删除已移入系统回收站：应用内无回收站还原路径，整批拒绝撤销
    // （用户可在系统回收站手动恢复）
    if logs.iter().any(|l| l.operation_type == "delete") {
        return Err(AppError::Forbidden("删除批次暂不支持撤销".into()));
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let mut undone_count = 0u32;
    let mut failed_count = 0u32;
    // 路径被移回的文件（file_id, 恢复的原路径），撤销后同步到向量索引
    let mut restored_paths: Vec<(String, String)> = Vec::new();

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
                                } else {
                                    restored_paths.push((rec.id.clone(), log.source_path.clone()));
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

    // 尽力而为：把移回原路径的文件同步到向量索引（失败仅告警）
    spawn_index_path_sync(state, restored_paths);

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

/// `/index/delete_by_file_ids` 请求体（对齐 sidecar）。
#[derive(Debug, Serialize)]
struct SidecarDeleteByFileIdsRequest {
    table_name: String,
    file_ids: Vec<String>,
}

/// 移除目录响应体。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoveDirectoryResponse {
    /// 从索引中移除的文件数。
    #[specta(type = specta_typescript::Number)]
    pub removed_files: i64,
}

/// 列出所有已扫描目录（含每个目录的文件数）。
///
/// # Errors
///
/// DB 锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn list_scanned_directories(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::db::ScannedDirectory>, String> {
    let guard = state.db.lock().map_err(|e| format!("DB 锁中毒: {e}"))?;
    ScannedDirectoryRepo::list_with_file_count(guard.conn()).map_err(|e| e.to_string())
}

/// 移除目录：软删该目录下所有文件 + 清理向量索引 + 删除目录记录。
///
/// 不删除磁盘文件，仅从 `FileMind` 索引中移除。重新扫描该目录即可恢复。
///
/// 流程：
///   1. 路径安全校验
///   2. 获取该目录下所有未软删除文件的 ID
///   3. best-effort 调 sidecar 清理对应向量（失败仅告警）
///   4. 软删该目录下所有文件，并同步清空其索引状态标记（防重扫后误跳过）
///   5. 删除目录记录
///
/// # Errors
///
/// 路径未通过安全校验或 DB 操作失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn remove_directory(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<RemoveDirectoryResponse, String> {
    remove_directory_inner(&path, &state).map_err(|e| e.to_string())
}

/// `remove_directory` 纯逻辑入口（便于测试）。
fn remove_directory_inner(path: &str, state: &AppState) -> AppResult<RemoveDirectoryResponse> {
    // 1. 路径安全校验
    let _safe_path = security::validate(path)?;

    // 2. 获取该目录下所有未软删除文件的 ID（用于清理向量）
    let file_ids = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::get_ids_by_path_prefix(guard.conn(), path)?
    };

    // 3. best-effort 清理向量索引（sidecar 未就绪或失败仅告警）
    if !file_ids.is_empty() {
        spawn_index_delete_by_file_ids(state, file_ids.clone());
    }

    // 4. 软删该目录下所有文件 + 同步清空索引状态标记。
    //    marker 必须清空：否则向量已删但标记仍在，之后重新扫描同一目录时，
    //    增量索引会把复活文件误判为「已建」而跳过（见 FileRepo::clear_embedding_marker）。
    let removed_files = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let removed = FileRepo::soft_delete_by_path_prefix(guard.conn(), path)?;
        if let Err(e) = FileRepo::clear_embedding_marker(guard.conn(), &file_ids) {
            log::warn!("移除目录：清空索引状态标记失败（不影响移除结果）: {e}");
        }
        drop(guard);
        removed
    };

    // 5. 删除目录记录
    {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        ScannedDirectoryRepo::delete(guard.conn(), path)?;
    }

    Ok(RemoveDirectoryResponse { removed_files })
}

/// 尽力而为：从向量索引中删除指定文件的全部向量行。
///
/// 与 `spawn_index_path_sync` 同模式：异步 spawn，失败仅告警，不影响主流程。
fn spawn_index_delete_by_file_ids(state: &AppState, file_ids: Vec<String>) {
    if file_ids.is_empty() {
        return;
    }

    let table_name = match state.db.lock() {
        Ok(guard) => match ConfigRepo::get(guard.conn()) {
            Ok(config) => format!("documents_{}_v1", config.embedding_model),
            Err(e) => {
                log::warn!("索引向量清理：读取 embedding 模型失败（跳过）: {e}");
                return;
            }
        },
        Err(poisoned) => {
            log::warn!("索引向量清理：获取 DB 锁中毒（跳过）: {poisoned}");
            return;
        }
    };

    let psk = match state.sidecar_psk.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("索引向量清理：获取 PSK 锁中毒（跳过）: {poisoned}");
            return;
        }
    };
    let Some(psk) = psk else {
        return;
    };
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let count = file_ids.len();
    tauri::async_runtime::spawn(async move {
        let request = SidecarDeleteByFileIdsRequest {
            table_name,
            file_ids,
        };
        let body = match serde_json::to_string(&request) {
            Ok(body) => body,
            Err(e) => {
                log::warn!("索引向量清理：序列化请求失败（跳过）: {e}");
                return;
            }
        };
        match proxy::forward_post("/index/delete_by_file_ids", &body, &psk, seq).await {
            Ok(_) => log::info!("索引向量已清理 {count} 个文件"),
            Err(e) => log::warn!("索引向量清理失败（不影响移除结果）: {e}"),
        }
    });
}

// 测试模块：1202 行内联测试按职责拆到 5 个文件（rules/complexity.md 的 600 行测试阈值）。
// 公共夹具在 file_ops_test_support；每个模块各自 #[cfg(test)]，不得只在首个上标注——
// 否则其余模块会进非测试构建，并因引用 cfg(test) 模块而编译失败。
#[cfg(test)]
#[path = "file_ops_test_support.rs"]
mod file_ops_test_support;

#[cfg(test)]
#[path = "file_ops_scan_tests.rs"]
mod file_ops_scan_tests;

#[cfg(test)]
#[path = "file_ops_preview_tests.rs"]
mod file_ops_preview_tests;

#[cfg(test)]
#[path = "file_ops_execute_tests.rs"]
mod file_ops_execute_tests;

#[cfg(test)]
#[path = "file_ops_undo_tests.rs"]
mod file_ops_undo_tests;
