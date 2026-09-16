//! 预览（dry-run）：按冲突策略生成操作计划与汇总，不做任何文件系统变更。

use crate::db::models::FileRecord;
use crate::db::FileRepo;
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::conflict_resolver::{self, ConflictStrategy, ConflictType, PlanStatus};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
pub(super) fn preview_operations_inner(
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
