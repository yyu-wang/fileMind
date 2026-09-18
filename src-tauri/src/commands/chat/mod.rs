//! RAG 对话命令：经 Sidecar `/chat/stream` SSE 代理流式问答。
//!
//! Rust 作为唯一出口：前端不能直连 Sidecar（PSK 在 Rust Keychain），
//! `chat_stream` 发起带 HMAC 签名的流式请求，后台任务逐帧解析 SSE 并
//! 通过 `chat://event` 事件推给前端。立即返回 `Ok`，流在后台推送。
//!
//! 模块划分（原单文件 465 行，逼近 Rust 模块 500 行强制阈值，见 `rules/complexity.md`）：
//!   - `fts`    FTS5 命中正文注入（Sidecar 永不碰 SQLite）
//!   - `stream` SSE 逐帧代理与双时限看门狗（BE-m8）
//!
//! 对外 IPC 契约（specta 导出的 `ChatStreamRequest` / `ChatTurn` /
//! `ChatChunkInput`）与命令入口留在本模块。

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::events::emit_chat_event;
use crate::AppState;

mod fts;
mod stream;

use fts::populate_fts_chunks;
use stream::stream_chat;

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
    /// 生成模型名。云端模式下若 `cloud_model` 非空，会被覆盖。
    pub llm_model: String,
    /// 云端推理模型名（仅 cloud 模式生效；空串则回落到 Provider 默认）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cloud_model: String,
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

/// 对话流式命令的默认请求体字段（RAG 问答默认参数）。
impl Default for ChatStreamRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            history: Vec::new(),
            table_name: String::new(),
            embedding_model: "bge-large-zh-v1.5".to_string(),
            inference_mode: "local".to_string(),
            llm_model: "qwen3.8-27b".to_string(),
            cloud_model: String::new(),
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
) -> Result<f64, String> {
    chat_stream_inner(app, &state, request)
        .map(|seq| {
            // specta 禁止导出 u64（BigInt 精度）；seq 是会话内单调计数器，
            // 远小于 2^52，转 f64 无精度损失（JS 安全整数范围）
            #[allow(clippy::cast_precision_loss)]
            {
                seq as f64
            }
        })
        .map_err(|e| e.to_string())
}

/// 对话流式命令的纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
fn chat_stream_inner(
    app: tauri::AppHandle,
    state: &AppState,
    mut request: ChatStreamRequest,
) -> AppResult<u64> {
    // 云端模式：若用户指定了 cloud_model，用它覆盖 llm_model
    // （云端 Provider 按 llm_model 前缀路由，所以 llm_model 即云端模型名）
    if request.inference_mode.eq_ignore_ascii_case("cloud") && !request.cloud_model.is_empty() {
        request.llm_model.clone_from(&request.cloud_model);
        request.cloud_model = String::new();
    }

    // FTS5 检索：在 Rust 层执行全文搜索，将命中文件内容注入 fts_chunks，
    // 供 Sidecar 做 RRF 融合检索（向量 + 关键词）。
    populate_fts_chunks(state, &mut request)?;

    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state.request_seq.fetch_add(1, Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_chat(app.clone(), psk, seq, request).await {
            emit_chat_event(&app, "error", error_event_payload(&e), seq);
        }
    });
    Ok(seq)
}

/// 后台流式任务失败时下发给前端的 error 事件载荷。
///
/// 独立成纯函数便于单测断言形状（code/message 契约）。
fn error_event_payload(e: &AppError) -> serde_json::Value {
    serde_json::json!({
        "code": "INTERNAL_ERROR",
        "message": e.to_string(),
    })
}

#[cfg(test)]
#[path = "../chat_tests.rs"]
mod tests;
