//! FTS5 命中正文注入（原 `commands/chat.rs` 拆出）。
//!
//! Sidecar 永不碰 SQLite，FTS5 由 Rust 层执行：命中文件正文在 Rust 侧读取后
//! 以 [`ChatChunkInput`] 形式注入请求体，供 Sidecar 做 RRF 融合检索。

use std::fs;
use std::io::Read;
use std::path::Path;

use super::{ChatChunkInput, ChatStreamRequest};
use crate::db::file_search::FileSearch;
use crate::error::{AppError, AppResult};
use crate::AppState;

/// FTS5 单文件读取上限（50MB，对齐 `MAX_FTS_FILE_BYTES` 与 Python `MAX_FILE_BYTES`；
/// 超大正文注入会放大请求体，Sidecar 侧 `MAX_BODY_SIZE` 已同步放宽到 64MB）。
const MAX_CHAT_FTS_BYTES: u64 = 50 * 1024 * 1024;

/// FTS5 全文搜索并填充 `fts_chunks`：读取命中文件正文，注入请求体。
///
/// Sidecar 永不碰 SQLite，FTS5 由 Rust 层执行。命中文件的正文在 Rust 侧读取后
/// 以 `ChatChunkInput` 形式注入 `request.fts_chunks`，供 Sidecar 的混合检索
/// （RRF 融合）使用。
///
/// # 降级策略
///
/// - FTS5 无命中 → `fts_chunks` 保持空数组（后续 Sidecar 纯向量检索兜底）
/// - 文件不可读 / 超大 → 跳过，不中断整体流程
/// - 数据库锁失败 → 直接返回错误（非降级）
///
/// # Errors
///
/// 数据库锁中毒或 FTS5 查询失败时返回错误。
pub(super) fn populate_fts_chunks(
    state: &AppState,
    request: &mut ChatStreamRequest,
) -> AppResult<()> {
    let fts_results = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        FileSearch::search(guard.conn(), &request.query, 20)?
    };

    if fts_results.is_empty() {
        return Ok(());
    }

    let mut chunks: Vec<ChatChunkInput> = Vec::with_capacity(fts_results.len());
    for result in &fts_results {
        let path = Path::new(&result.file.path);
        if !path.is_file() {
            continue;
        }

        let Ok(metadata) = fs::metadata(path) else {
            continue;
        };

        if metadata.len() > MAX_CHAT_FTS_BYTES {
            continue;
        }

        let Ok(mut file_handle) = fs::File::open(path) else {
            continue;
        };

        let mut contents = String::new();
        if file_handle.read_to_string(&mut contents).is_err() {
            continue;
        }

        if contents.trim().is_empty() {
            continue;
        }

        chunks.push(ChatChunkInput {
            chunk_id: result.file.id.clone(),
            text: contents,
            file_path: result.file.path.clone(),
            page: 0,
        });
    }

    if !chunks.is_empty() {
        request.fts_chunks = chunks;
    }

    Ok(())
}
