//! 智能分类命令：生成分类预览计划（规则引擎 + 启发式），不执行文件系统变更。
//!
//! 执行阶段复用 T3.3 `execute_operations` / `undo_batch`（分类计划项转 `PlanItem`，
//! 携带各自目标路径 + `ConflictStrategy::Skip`），共享链式撤销，无需重复实现执行链路。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::models::FileRecord;
use crate::db::{CategoryRepo, FileRepo, RuleRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::classifier::{self, ClassifyPreview};
use crate::sidecar::proxy;
use crate::AppState;

/// 生成分类预览（设计稿 §5.1）。
///
/// 流程：
///   1. `security::validate` 校验扫描根（须存在且不在黑名单）
///   2. `file_ids` 非空；`FileRepo::get_by_ids` 反查文件
///   3. `RuleRepo::list_enabled`（优先级降序）+ `CategoryRepo::list_all`
///   4. `classifier::generate_plan` 生成计划 → `aggregate_stats` 聚合统计
///   5. 规则/启发式未命中的待确认项 → sidecar `/classify` LLM 兜底补全
///      （T6.11；sidecar 不可用时降级为保持待确认，不阻塞预览）
///   6. 生成 `batch_id`（uuid4，供执行阶段接力）
///
/// # Errors
///
/// `scan_root` 非法返回 `UnsafePath`；`file_ids` 为空或全部无效返回 `InvalidInput`；
/// 数据库读取失败返回 `Database`。
#[tauri::command]
#[specta::specta]
pub async fn classify_preview(
    state: tauri::State<'_, AppState>,
    file_ids: Vec<String>,
    scan_root: String,
) -> Result<ClassifyPreview, String> {
    classify_preview_inner(&state, &file_ids, &scan_root)
        .await
        .map_err(|e| e.to_string())
}

/// 分类预览纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
async fn classify_preview_inner(
    state: &AppState,
    file_ids: &[String],
    scan_root: &str,
) -> AppResult<ClassifyPreview> {
    if file_ids.is_empty() {
        return Err(AppError::InvalidInput("file_ids 不能为空".to_string()));
    }

    let safe_root = security::validate(scan_root)?;
    // 同级收纳根（`<扫描根名>_已分类`），分类目标基于它拼接并回传前端
    let output_root = unique_output_root(&safe_root)?;

    let (files, rules, categories) = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let files = FileRepo::get_by_ids(guard.conn(), file_ids)?;
        let rules = RuleRepo::list_enabled(guard.conn())?;
        let categories = CategoryRepo::list_all(guard.conn())?;
        drop(guard);
        (files, rules, categories)
    };

    if files.is_empty() {
        return Err(AppError::InvalidInput("指定文件不存在或已失效".to_string()));
    }

    let mut items = classifier::generate_plan(&files, &safe_root, &rules, &categories)?;

    // —— T6.11：规则/启发式未命中的待确认项 → sidecar LLM 兜底 ——
    // 失败降级：sidecar 未就绪 / 请求失败 / 响应解析失败 → 保持待确认，不阻塞预览
    // （与 scan_directory 的「UI 优先」策略一致，本地分类结果始终返回）
    let pending_ids: Vec<String> = items
        .iter()
        .filter(|i| i.rule_source == classifier::PENDING_SOURCE)
        .map(|i| i.file_id.clone())
        .collect();
    if !pending_ids.is_empty() {
        match llm_classify_fallback(
            state,
            &files,
            &mut items,
            &pending_ids,
            &safe_root,
            &categories,
        )
        .await
        {
            Ok(n) => {
                if n > 0 {
                    log::info!("LLM 兜底补充分类：{n} 个待确认文件");
                }
            }
            Err(e) => {
                log::warn!("LLM 兜底不可用，保持待确认（不影响预览）: {e}");
            }
        }
    }

    let preview_stats = classifier::aggregate_stats(&items);
    let batch_id = uuid::Uuid::new_v4().to_string();

    Ok(ClassifyPreview {
        batch_id,
        output_root: output_root.to_string_lossy().to_string(),
        items,
        stats: preview_stats,
    })
}

/// 计算本次分类的收纳根：以同级 `<扫描根名>_已分类` 为基准。同名目录已存在时
/// 视为已建收纳目录并**复用**（支持分批整理同一扫描根）；仅当该路径被同名普通
/// 文件占用（非目录）时报错，避免执行期 `create_dir_all` 静默失败。
///
/// # Errors
///
/// 扫描根无法生成收纳目录名（文件系统根等），或同级路径被同名普通文件占用时返回 `InvalidInput`。
fn unique_output_root(scan_root: &Path) -> AppResult<PathBuf> {
    let base = classifier::sibling_output_root(scan_root)?;
    if base.exists() && !base.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "同级收纳目录名被同名文件占用：{}，请移动该文件后重试",
            base.display()
        )));
    }
    Ok(base)
}

// ----------------------------------------------------------------------
// T6.11：LLM 兜底（sidecar /classify 代理调用）
// ----------------------------------------------------------------------

/// `/classify` 请求体（对齐 sidecar `ClassifyItem`，只传元数据，内容摘要在范围外）。
#[derive(Debug, Serialize)]
struct SidecarClassifyItem {
    name: String,
    extension: String,
    path: String,
    size: i64,
}

/// `/classify` 请求体（对齐 sidecar `ClassifyRequest`）。
#[derive(Debug, Serialize)]
struct SidecarClassifyRequest {
    files: Vec<SidecarClassifyItem>,
    categories: Vec<String>,
}

/// `/classify` 响应单条（只取合并所需字段；status 对齐 sidecar 枚举）。
#[derive(Debug, Deserialize)]
struct SidecarClassifyItemResult {
    file_name: String,
    category: String,
    status: String,
}

/// `/classify` 响应体。
#[derive(Debug, Deserialize)]
struct SidecarClassifyResponse {
    items: Vec<SidecarClassifyItemResult>,
}

/// 对待确认文件执行 LLM 兜底分类，返回成功补充分类的文件数。
///
/// 流程：pending 文件元数据 → POST /classify（HMAC 签名，复用 `proxy::forward_post`）
/// → 解析响应 → `classifier::apply_llm_fallback` 合并进 plan。
///
/// # Errors
///
/// PSK 未就绪返回 `SidecarUnavailable`；网络/解析失败透传（调用方降级）。
async fn llm_classify_fallback(
    state: &AppState,
    files: &[FileRecord],
    items: &mut [classifier::ClassifyPlanItem],
    pending_ids: &[String],
    scan_root: &Path,
    categories: &[crate::db::models::Category],
) -> AppResult<usize> {
    // 按 file_id 筛出待确认项对应的文件记录（含 size，供 sidecar 元数据）
    let pending_files: Vec<&FileRecord> = files
        .iter()
        .filter(|f| pending_ids.contains(&f.id))
        .collect();
    if pending_files.is_empty() {
        return Ok(0);
    }

    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // 构造请求体：只传 pending 项；categories 供 P-01 预定义分类列表
    let request = SidecarClassifyRequest {
        files: pending_files
            .iter()
            .map(|f| SidecarClassifyItem {
                name: f.file_name.clone(),
                extension: Path::new(&f.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_string(),
                path: f.path.clone(),
                size: f.file_size,
            })
            .collect(),
        categories: categories.iter().map(|c| c.name.clone()).collect(),
    };

    let body = serde_json::to_string(&request)?;
    let resp = proxy::forward_post("/classify", &body, &psk, seq).await?;
    let parsed: SidecarClassifyResponse = serde_json::from_str(&resp)?;

    // file_name → (category, status)；sidecar 不回显 file_id，按文件名反查 plan
    let llm_map: HashMap<String, (String, String)> = parsed
        .items
        .into_iter()
        .map(|i| (i.file_name, (i.category, i.status)))
        .collect();

    let before = items
        .iter()
        .filter(|i| i.rule_source == classifier::PENDING_SOURCE)
        .count();
    classifier::apply_llm_fallback(items, &llm_map, scan_root, categories)?;
    let after = items
        .iter()
        .filter(|i| i.rule_source == classifier::PENDING_SOURCE)
        .count();
    Ok(before.saturating_sub(after))
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
// 测试移到兄弟文件 classify_tests.rs（原内嵌，与实现合计超 Rust 模块 300 行警告阈值；
// 与 db/file_repo_tests.rs、db/rule_repo_tests.rs 同款）。

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
