//! 索引命令：建立文件索引（Rust 读 SQLite → sidecar /index/build 向量化 → LanceDB）。
//!
//! 问答链路的数据源：文件扫描只进 SQLite，问答检索需要向量索引（LanceDB）。
//! 本命令把全部未删除文件交给 sidecar 批量向量化写入，建立后可进行 RAG 问答。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::file_search::FileSearch;
use crate::db::{ConfigRepo, FileRepo};
use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use crate::AppState;

/// 全量索引的文件上限（支撑十万级库；真实场景远小于此）。
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

/// 建立文件索引：SQLite 全量未删除文件 → sidecar `/index/build` 向量化写入 `LanceDB`。
///
/// `embedding_model` 与目标表名来自 `app_config`（对齐问答请求 `documents_{model}_v1`）。
///
/// # Errors
///
/// Sidecar 未就绪、配置读取失败或 sidecar 返回错误时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn build_index(state: State<'_, AppState>) -> Result<IndexBuildResponse, String> {
    build_index_inner(&state).await.map_err(|e| e.to_string())
}

/// 建索引纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
async fn build_index_inner(state: &AppState) -> AppResult<IndexBuildResponse> {
    // 1. 读取配置：embedding 模型 + 目标表名（对齐问答 `documents_{model}_v1`）
    let (embedding_model, table_name) = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let config = ConfigRepo::get(guard.conn())?;
        let model = config.embedding_model;
        let table = format!("documents_{model}_v1");
        drop(guard);
        (model, table)
    };

    // 2. 全量读取未删除文件
    let files = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileRepo::list(guard.conn(), None, 0, INDEX_FILE_LIMIT)?
    };
    if files.is_empty() {
        return Ok(IndexBuildResponse {
            indexed_count: 0,
            skipped_count: 0,
        });
    }

    // 3. 填充 FTS5 content 列（文件正文入索引，支撑关键词检索）
    let (fts_indexed, fts_skipped) = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileSearch::populate_fts_content(guard.conn(), &files)?
    };

    // 4. 构造请求 → HMAC 代理调 sidecar /index/build（向量索引）
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let request = SidecarIndexBuildRequest {
        files: files
            .iter()
            .map(|f| SidecarIndexBuildFile {
                file_id: f.id.clone(),
                path: f.path.clone(),
            })
            .collect(),
        embedding_model,
        table_name,
    };
    let body = serde_json::to_string(&request)?;
    let resp = proxy::forward_post("/index/build", &body, &psk, seq).await?;
    let parsed: IndexBuildResponse = serde_json::from_str(&resp)?;

    // 合并 FTS5 + LanceDB 结果：取较大值（两部分成功即可）
    let indexed_count = parsed.indexed_count.max(fts_indexed);
    let skipped_count = parsed.skipped_count + fts_skipped;

    Ok(IndexBuildResponse {
        indexed_count,
        skipped_count,
    })
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
