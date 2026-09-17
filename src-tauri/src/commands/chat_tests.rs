//! `chat` 命令的单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `commands/chat/mod.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。
//! 注意 `super` 指 `commands::chat` 模块，不是文件所在目录 `commands`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::stream::{next_stream_chunk, StreamTimeout, STREAM_IDLE_TIMEOUT, STREAM_TOTAL_TIMEOUT};
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
    let chunk: ChatChunkInput = serde_json::from_str(r#"{"chunk_id":"c1","text":"你好"}"#).unwrap();
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
