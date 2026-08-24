//! RAG 对话命令：经 Sidecar `/chat/stream` SSE 代理流式问答。
//!
//! Rust 作为唯一出口：前端不能直连 Sidecar（PSK 在 Rust Keychain），
//! `chat_stream` 发起带 HMAC 签名的流式请求，后台任务逐帧解析 SSE 并
//! 通过 `chat://event` 事件推给前端。立即返回 `Ok`，流在后台推送。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

use crate::error::{AppError, AppResult};
use crate::events::emit_chat_event;
use crate::sidecar::proxy;
use crate::sidecar::sse::SseParser;
use crate::AppState;

/// 一轮对话历史（P-02 查询改写输入），对齐 Sidecar `ChatTurn`。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChatTurn {
    /// 用户提问。
    pub user: String,
    /// 助手回答。
    pub assistant: String,
}

/// FTS5 命中（Rust 层从 `SQLite` 提供，含原文文本），对齐 Sidecar `ChatChunkInput`。
///
/// 当前桌面端尚无 chunk 级 `FTS5` 表（建索引为后续独立任务），请求中恒为空。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ChatChunkInput {
    /// 命中 chunk 标识。
    pub chunk_id: String,
    /// 命中原文文本（FTS-only 补全生成上下文）。
    pub text: String,
    /// 所属文件路径。
    #[serde(default)]
    pub file_path: String,
    /// 所属页码。
    #[serde(default)]
    pub page: i32,
}

/// `/chat/stream` 请求体，默认值对齐 Sidecar `ChatStreamRequest`。
///
/// 仅 `query` / `table_name` 必填，其余字段缺失时由 serde 填充
/// `Default`（见 [`ChatStreamRequest::default`]），与 Sidecar 端默认一致。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct ChatStreamRequest {
    /// 用户提问（含对话历史中的代词消解由 Sidecar 处理）。
    pub query: String,
    /// 最近 3 轮对话历史（对齐 P-02 `HISTORY_TURNS=3`）。
    pub history: Vec<ChatTurn>,
    /// 目标 `LanceDB` 表名（`documents_{embedding_model}_v{version}`）。
    pub table_name: String,
    /// 向量化模型名。
    pub embedding_model: String,
    /// 推理模式（`local` / `cloud` / `hybrid`）。
    pub inference_mode: String,
    /// 生成模型名。
    pub llm_model: String,
    /// 向量检索候选数。
    pub top_k: u32,
    /// 重排后保留数。
    pub rerank_top_k: u32,
    /// P-04 自我纠正最大重试次数。
    pub max_retries: u32,
    /// FTS5 命中（当前恒空，见 [`ChatChunkInput`]）。
    pub fts_chunks: Vec<ChatChunkInput>,
    /// 会话标识（多轮会话透传，无则为 `None`）。
    pub session_id: Option<String>,
}

impl Default for ChatStreamRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            history: Vec::new(),
            table_name: String::new(),
            embedding_model: "bge-large-zh-v1.5".to_string(),
            inference_mode: "local".to_string(),
            llm_model: "qwen3.8-27b".to_string(),
            top_k: 20,
            rerank_top_k: 5,
            max_retries: 2,
            fts_chunks: Vec::new(),
            session_id: None,
        }
    }
}

/// 发起 RAG 对话流式请求，SSE 帧经 `chat://event` 逐帧推给前端。
///
/// 命令本身立即返回 `Ok`：真正的流式消费在后台任务中执行，
/// 避免阻塞 IPC 调用方。Sidecar 进程不可用（PSK 未就绪）时返回错误。
///
/// # Errors
///
/// Sidecar 未握手（PSK 为 `None`）时返回 `SidecarUnavailable`。
#[tauri::command(async)]
#[specta::specta]
pub fn chat_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: ChatStreamRequest,
) -> Result<(), String> {
    chat_stream_inner(app, &state, request).map_err(|e| e.to_string())
}

/// 对话流式命令的纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
fn chat_stream_inner(
    app: tauri::AppHandle,
    state: &AppState,
    request: ChatStreamRequest,
) -> AppResult<()> {
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state.request_seq.fetch_add(1, Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_chat(app.clone(), psk, seq, request).await {
            emit_chat_event(
                &app,
                "error",
                serde_json::json!({
                    "code": "INTERNAL_ERROR",
                    "message": e.to_string(),
                }),
            );
        }
    });
    Ok(())
}

/// 迭代 Sidecar SSE 响应体，逐帧解析并推送 `chat://event`。
///
/// 流中途网络错误时返回 `Network`，由调用方补发 `error` 帧。
async fn stream_chat(
    app: tauri::AppHandle,
    psk: Vec<u8>,
    seq: u64,
    request: ChatStreamRequest,
) -> AppResult<()> {
    const PATH: &str = "/chat/stream";
    let body = serde_json::to_string(&request)?;
    let resp = proxy::forward_post_stream(PATH, &body, &psk, seq).await?;
    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::default();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(AppError::Network)?;
        for frame in parser.feed(&bytes) {
            emit_chat_event(&app, &frame.event, frame.data);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// 反序列化缺省字段 → 填充 Sidecar 端默认值（对齐 `ChatStreamRequest`）。
    #[test]
    fn deserialize_fills_sidecar_defaults() {
        let req: ChatStreamRequest = serde_json::from_str(
            r#"{"query":"营收多少","table_name":"documents_bge-large-zh-v1.5_v1"}"#,
        )
        .unwrap();
        assert_eq!(req.embedding_model, "bge-large-zh-v1.5");
        assert_eq!(req.inference_mode, "local");
        assert_eq!(req.llm_model, "qwen3.8-27b");
        assert_eq!(req.top_k, 20);
        assert_eq!(req.rerank_top_k, 5);
        assert_eq!(req.max_retries, 2);
        assert!(req.history.is_empty());
        assert!(req.fts_chunks.is_empty());
        assert_eq!(req.session_id, None);
    }

    /// 序列化 → 字段名与 Sidecar `ChatStreamRequest` 一致（HTTP 层契约）。
    #[test]
    fn serialize_preserves_sidecar_field_names() {
        let req = ChatStreamRequest {
            query: "本地模式有什么优势？".to_string(),
            history: vec![ChatTurn {
                user: "支持哪些模式？".to_string(),
                assistant: "本地和云端".to_string(),
            }],
            table_name: "documents_bge-large-zh-v1.5_v1".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["query"], "本地模式有什么优势？");
        assert_eq!(json["embedding_model"], "bge-large-zh-v1.5");
        assert_eq!(json["llm_model"], "qwen3.8-27b");
        assert_eq!(json["max_retries"], 2);
        assert_eq!(json["history"][0]["user"], "支持哪些模式？");
        assert_eq!(json["fts_chunks"], serde_json::json!([]));
    }

    /// `ChatChunkInput` 可选字段默认值对齐 Sidecar。
    #[test]
    fn chunk_input_defaults() {
        let chunk: ChatChunkInput =
            serde_json::from_str(r#"{"chunk_id":"c1","text":"你好"}"#).unwrap();
        assert_eq!(chunk.file_path, "");
        assert_eq!(chunk.page, 0);
    }
}
