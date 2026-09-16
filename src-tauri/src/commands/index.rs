//! 索引命令：建立文件索引（Rust 读 SQLite → sidecar /index/build 向量化 → LanceDB）。
//!
//! 问答链路的数据源：文件扫描只进 SQLite，问答检索需要向量索引（LanceDB）。
//! 本命令把**待向量化**的未删除文件交给 sidecar 批量向量化写入，建立后可进行 RAG 问答。
//!
//! 增量语义：只处理「从未建过 / Embedding 模型切换 / 内容发生变更」的文件，
//! 建成后把 `embedding_model` + `embedding_hash` 回写到 `files` 表（见
//! `FileRepo::mark_embedded`），下次点「建立索引」自动跳过已建且未变的文件。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::embedding_table;
use crate::db::file_search::FileSearch;
use crate::db::models::FileRecord;
use crate::db::{ConfigRepo, FileRepo};
use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use crate::AppState;

/// 单次索引的文件上限（支撑十万级库；真实场景远小于此）。
const INDEX_FILE_LIMIT: i64 = 1_000_000;

/// `/index/build` 请求体（对齐 sidecar `IndexBuildRequest`）。
#[derive(Debug, Serialize)]
struct SidecarIndexBuildFile {
    file_id: String,
    path: String,
}

/// `/index/build` 请求体。
#[derive(Debug, Serialize)]
struct SidecarIndexBuildRequest {
    files: Vec<SidecarIndexBuildFile>,
    embedding_model: String,
    table_name: String,
}

/// `/index/build` 响应体（含建成文件清单；对齐 sidecar `IndexBuildResponse`）。
#[derive(Debug, Deserialize)]
struct SidecarIndexBuildResponse {
    /// 成功索引的文件数。
    indexed_count: i64,
    /// 跳过的文件数（非文本 / 读取失败 / 空内容）。
    skipped_count: i64,
    /// 实际写入向量的 `file_id`（供回写 `SQLite` 索引状态标记；旧版 sidecar 无此字段时默认为空）。
    #[serde(default)]
    indexed_file_ids: Vec<String>,
}

/// `/index/build` 响应体（对齐 sidecar `IndexBuildResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct IndexBuildResponse {
    /// 成功索引的文件数。
    #[specta(type = specta_typescript::Number)]
    pub indexed_count: i64,
    /// 跳过的文件数（非文本 / 读取失败 / 空内容）。
    #[specta(type = specta_typescript::Number)]
    pub skipped_count: i64,
}

/// 建立文件索引：SQLite 待向量化文件 → sidecar `/index/build` 向量化写入 `LanceDB`。
///
/// 增量判定（[`FileRepo::list_pending_embedding`]）：只取「从未建过索引 /
/// Embedding 模型切换 / 内容变更（`embedding_hash != content_hash`）」的文件，
/// 已建且未变的直接跳过，不再全量重算。向量化成功后回写索引状态标记。
///
/// `embedding_model` 与目标表名来自 `app_config`（对齐问答请求 `documents_{model}_v1`）。
///
/// # Errors
///
/// Sidecar 未就绪、配置读取失败、标记回写失败或 sidecar 返回错误时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn build_index(state: State<'_, AppState>) -> Result<IndexBuildResponse, String> {
    build_index_inner(&state).await.map_err(|e| e.to_string())
}

/// 建索引纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
async fn build_index_inner(state: &AppState) -> AppResult<IndexBuildResponse> {
    // 1. 读取配置：当前 Embedding 模型（表名留到拿到 PSK 后再解析——版本号来自
    //    Sidecar 注册表，见 `commands::embedding_table`，此处不再拼表名）
    let embedding_model = load_embedding_model(state)?;

    // 2. 只取「待向量化」文件：未建过 / 换模型 / 内容变更才入选（增量核心）
    let files = load_pending_files(state, &embedding_model)?;
    if files.is_empty() {
        return Ok(IndexBuildResponse {
            indexed_count: 0,
            skipped_count: 0,
        });
    }

    // 3. 填充 FTS5 content 列（仅候选文件；已建文件的 FTS 正文保留不动）
    let (fts_indexed, fts_skipped) = populate_fts(state, &files)?;

    // 4. 构造请求 → HMAC 代理调 sidecar /index/build（向量索引）
    let psk = load_psk(state)?;
    // 表名解析自身要发一次探测请求（消耗一个序号），索引请求另取一个——
    // Sidecar 中间件要求序号严格递增，复用同一序号会被判重放。
    let seq_probe = next_seq(state);
    let seq = next_seq(state);
    let table_name =
        embedding_table::resolve_vector_table_parts(&state.db, &psk, seq_probe).await?;
    let parsed = forward_index_build(&files, &embedding_model, table_name, &psk, seq).await?;

    // 5. 向量化成功后回写索引状态标记。只标记「实际写入向量」的文件
    //    （sidecar 返回的 indexed_file_ids）；读取失败 / 非文本文件保持未标记，
    //    下次点击自动重试。content_hash 为 None 的文件无哈希可比，同样跳过标记。
    let mark_entries = build_mark_entries(&files, &parsed.indexed_file_ids);
    if !mark_entries.is_empty() {
        mark_embedded(state, &embedding_model, &mark_entries)?;
    }

    // 合并 FTS5 + LanceDB 结果：取较大值（两部分成功即可）
    Ok(IndexBuildResponse {
        indexed_count: parsed.indexed_count.max(fts_indexed),
        skipped_count: parsed.skipped_count + fts_skipped,
    })
}

/// 读取当前配置的 Embedding 模型名。
fn load_embedding_model(state: &AppState) -> AppResult<String> {
    let guard = state
        .db
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
    Ok(ConfigRepo::get(guard.conn())?.embedding_model)
}

/// 取「待向量化」候选文件（未建过 / 换模型 / 内容变更），上限 `INDEX_FILE_LIMIT`。
fn load_pending_files(state: &AppState, embedding_model: &str) -> AppResult<Vec<FileRecord>> {
    let guard = state
        .db
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
    FileRepo::list_pending_embedding(guard.conn(), embedding_model, INDEX_FILE_LIMIT)
}

/// 填充候选文件的 FTS5 正文，返回（已填、跳过）计数。
fn populate_fts(state: &AppState, files: &[FileRecord]) -> AppResult<(i64, i64)> {
    let guard = state
        .db
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
    FileSearch::populate_fts_content(guard.conn(), files)
}

/// 取当前 PSK（Sidecar 未就绪时报错）。
fn load_psk(state: &AppState) -> AppResult<Vec<u8>> {
    state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))
}

/// 取下一个请求序号（Sidecar 中间件要求严格递增，复用会被判重放）。
fn next_seq(state: &AppState) -> u64 {
    state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// 组装并发送 `/index/build` 请求，返回解析后的响应。
async fn forward_index_build(
    files: &[FileRecord],
    embedding_model: &str,
    table_name: String,
    psk: &[u8],
    seq: u64,
) -> AppResult<SidecarIndexBuildResponse> {
    let request = SidecarIndexBuildRequest {
        files: files
            .iter()
            .map(|f| SidecarIndexBuildFile {
                file_id: f.id.clone(),
                path: f.path.clone(),
            })
            .collect(),
        embedding_model: embedding_model.to_string(),
        table_name,
    };
    let body = serde_json::to_string(&request)?;
    let resp = proxy::forward_post("/index/build", &body, psk, seq).await?;
    Ok(serde_json::from_str(&resp)?)
}

/// 算出需要回写索引状态的（`file_id`, `content_hash`）条目。
///
/// 只覆盖 sidecar 明确回报「已写入向量」的文件；无哈希的文件无法比对，跳过。
fn build_mark_entries(files: &[FileRecord], indexed_file_ids: &[String]) -> Vec<(String, String)> {
    let hash_by_id: HashMap<&str, &str> = files
        .iter()
        .filter_map(|f| f.content_hash.as_deref().map(|h| (f.id.as_str(), h)))
        .collect();
    indexed_file_ids
        .iter()
        .filter_map(|id| {
            hash_by_id
                .get(id.as_str())
                .map(|h| (id.clone(), (*h).to_string()))
        })
        .collect()
}

/// 回写这些文件的索引状态标记（`embedding_model` + `embedding_hash`），返回受影响行数。
fn mark_embedded(
    state: &AppState,
    embedding_model: &str,
    entries: &[(String, String)],
) -> AppResult<usize> {
    let guard = state
        .db
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
    FileRepo::mark_embedded(guard.conn(), embedding_model, entries)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sidecar::SidecarManager;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    /// 构造最小可用 AppState（DB 指向临时 DB，Sidecar/PSK 用占位）。
    fn make_test_app_state(db_path: &std::path::Path) -> AppState {
        let db = Database::open(db_path).expect("打开测试 DB 失败");
        AppState {
            db: std::sync::Arc::new(Mutex::new(db)),
            sidecar_manager: Mutex::new(SidecarManager::new(
                "/dev/null/sidecar-nonexistent".into(),
            )),
            sidecar_psk: Mutex::new(None),
            sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
            sidecar_status: Mutex::new(crate::SidecarStatus::Starting),
        }
    }

    /// sidecar 不可用（PSK=None）时返回错误；空文件库时直接返回 0/0。
    #[tokio::test]
    async fn test_build_index_empty_library_returns_zero() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let result = build_index_inner(&state).await?;
        assert_eq!(result.indexed_count, 0);
        assert_eq!(result.skipped_count, 0);
        Ok(())
    }
}
