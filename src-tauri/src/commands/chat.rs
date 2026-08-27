//! RAG 对话命令：经 Sidecar `/chat/stream` SSE 代理流式问答。
//!
//! Rust 作为唯一出口：前端不能直连 Sidecar（PSK 在 Rust Keychain），
//! `chat_stream` 发起带 HMAC 签名的流式请求，后台任务逐帧解析 SSE 并
//! 通过 `chat://event` 事件推给前端。立即返回 `Ok`，流在后台推送。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::db::file_search::FileSearch;
use crate::error::{AppError, AppResult};
use crate::events::emit_chat_event;
use crate::sidecar::proxy;
use crate::sidecar::sse::SseParser;
use crate::AppState;

/// FTS5 单文件读取上限（1MB，避免大文件撑爆请求体）。
const MAX_CHAT_FTS_BYTES: u64 = 1_048_576;

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
fn populate_fts_chunks(state: &AppState, request: &mut ChatStreamRequest) -> AppResult<()> {
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

/// 后台流式任务失败时下发给前端的 error 事件载荷。
///
/// 独立成纯函数便于单测断言形状（code/message 契约）。
fn error_event_payload(e: &AppError) -> serde_json::Value {
    serde_json::json!({
        "code": "INTERNAL_ERROR",
        "message": e.to_string(),
    })
}

/// 流式空闲超时：连续 90s 无任何数据块视为流挂起（BE-m8）。
/// 与前端批次 7 看门狗（90s 空闲 + 600s 总量）数值对齐——后端先到先报
/// 真实原因，前端看门狗退化为纯 UI 兜底。
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// 流式总量上限（BE-m8）：防慢速滴流无限占用后台任务。
const STREAM_TOTAL_TIMEOUT: Duration = Duration::from_mins(10);

/// 流超时类型（BE-m8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamTimeout {
    /// 空闲超时（长时间无任何数据块）。
    Idle,
    /// 总量超时（整体时间上限）。
    Total,
}

/// 从流中取下一块，带空闲 + 总量双时限（BE-m8）。
///
/// - `Ok(Some(chunk))`：收到数据块（并重置空闲计时）
/// - `Ok(None)`：流正常结束
/// - `Err(timeout)`：空闲或总量超时
///
/// 网络错误经 `Ok(Some(Err(..)))` 原样透传，由调用方转 `AppError`。
/// 独立成泛型函数便于用 `#[tokio::test(start_paused = true)]` 单测超时行为。
async fn next_stream_chunk<S, T, E>(
    stream: &mut S,
    idle_deadline: &mut tokio::time::Instant,
    total_deadline: tokio::time::Instant,
) -> Result<Option<std::result::Result<T, E>>, StreamTimeout>
where
    S: futures_util::Stream<Item = std::result::Result<T, E>> + Unpin,
{
    let wake = (*idle_deadline).min(total_deadline);
    match tokio::time::timeout_at(wake, stream.next()).await {
        Ok(Some(chunk)) => {
            // 收到任意数据块（含 Err 块）都证明流仍活跃，重置空闲计时
            *idle_deadline = tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT;
            Ok(Some(chunk))
        }
        Ok(None) => Ok(None),
        Err(_) => {
            // timeout_at 在两个 deadline 中更早者到点；显式比较区分超时类型
            if total_deadline <= tokio::time::Instant::now() {
                Err(StreamTimeout::Total)
            } else {
                Err(StreamTimeout::Idle)
            }
        }
    }
}

/// 迭代 Sidecar SSE 响应体，逐帧解析并推送 `chat://event`。
///
/// 非 2xx 响应在 [`proxy::forward_post_stream`] 内已转为 Err（BE-C5），
/// 由调用方补发 error 帧；流中途网络错误同样返回 `Network`。
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
    // BE-m8：双时限看门狗——挂起的流不再让后台任务永驻；超时返回 Err，
    // 经 spawn 包装器发 error 事件（前端按 seq 匹配正常收尾复位）
    let total_deadline = tokio::time::Instant::now() + STREAM_TOTAL_TIMEOUT;
    let mut idle_deadline = tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT;
    loop {
        let chunk = match next_stream_chunk(&mut stream, &mut idle_deadline, total_deadline).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break, // 流正常结束
            Err(StreamTimeout::Idle) => {
                return Err(AppError::SidecarUnavailable(
                    "流式响应空闲超时（90s 无数据），已中止".to_string(),
                ));
            }
            Err(StreamTimeout::Total) => {
                return Err(AppError::SidecarUnavailable(
                    "流式响应总量超时（600s 上限），已中止".to_string(),
                ));
            }
        };
        let bytes = chunk.map_err(AppError::Network)?;
        for frame in parser.feed(&bytes) {
            emit_chat_event(&app, &frame.event, frame.data, seq);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// 反序列化缺省字段 → 填充 Sidecar 端默认值（对齐 `ChatStreamRequest`）。
    /// BE-m8：pending 流在空闲 deadline 到点返回 Idle 超时。
    #[tokio::test(start_paused = true)]
    async fn stream_chunk_idle_timeout_fires() {
        let mut stream =
            futures_util::stream::pending::<std::result::Result<&'static [u8], reqwest::Error>>();
        let mut idle = tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT;
        let total = tokio::time::Instant::now() + STREAM_TOTAL_TIMEOUT;
        let err = next_stream_chunk(&mut stream, &mut idle, total)
            .await
            .unwrap_err();
        assert_eq!(err, StreamTimeout::Idle);
    }

    /// BE-m8：收到数据块重置空闲计时——数据后挂起在新的空闲 deadline 超时。
    #[tokio::test(start_paused = true)]
    async fn stream_chunk_data_resets_idle_deadline() {
        use futures_util::stream::{pending, StreamExt};
        let stream =
            futures_util::stream::iter(vec![Ok::<&'static [u8], reqwest::Error>(&b"data"[..])])
                .chain(pending::<std::result::Result<&'static [u8], reqwest::Error>>());
        tokio::pin!(stream);
        let mut idle = tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT;
        let total = tokio::time::Instant::now() + STREAM_TOTAL_TIMEOUT;
        // 第一块立即到达
        let first = next_stream_chunk(&mut stream, &mut idle, total)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first, &b"data"[..]);
        // 之后挂起：在重置后的空闲 deadline 超时（而非旧 deadline）
        let err = next_stream_chunk(&mut stream, &mut idle, total)
            .await
            .unwrap_err();
        assert_eq!(err, StreamTimeout::Idle);
    }

    /// BE-m8：总量 deadline 先于空闲到点时返回 Total 超时。
    #[tokio::test(start_paused = true)]
    async fn stream_chunk_total_timeout_fires() {
        let mut stream =
            futures_util::stream::pending::<std::result::Result<&'static [u8], reqwest::Error>>();
        let now = tokio::time::Instant::now();
        // 构造 idle > total：更早的总量 deadline 先到
        let mut idle = now + STREAM_TOTAL_TIMEOUT * 2;
        let total = now + STREAM_IDLE_TIMEOUT;
        let err = next_stream_chunk(&mut stream, &mut idle, total)
            .await
            .unwrap_err();
        assert_eq!(err, StreamTimeout::Total);
    }

    /// BE-m8：流正常结束返回 Ok(None)，不触发超时。
    #[tokio::test(start_paused = true)]
    async fn stream_chunk_returns_none_on_end() {
        let mut stream = futures_util::stream::iter(Vec::<
            std::result::Result<&'static [u8], reqwest::Error>,
        >::new());
        let mut idle = tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT;
        let total = tokio::time::Instant::now() + STREAM_TOTAL_TIMEOUT;
        assert!(next_stream_chunk(&mut stream, &mut idle, total)
            .await
            .unwrap()
            .is_none());
    }

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

    /// error 事件载荷形状：code/message 契约（前端 error 分支依赖）。
    #[test]
    fn error_event_payload_shape() {
        let payload = error_event_payload(&AppError::SidecarUnavailable("连接中断".to_string()));
        assert_eq!(payload["code"], "INTERNAL_ERROR");
        assert!(payload["message"]
            .as_str()
            .is_some_and(|m| m.contains("连接中断")));
    }
}
