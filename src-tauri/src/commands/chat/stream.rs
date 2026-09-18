//! Sidecar SSE 流的逐帧代理与双时限看门狗（原 `commands/chat.rs` 拆出）。

use std::time::Duration;

use futures_util::StreamExt;

use super::ChatStreamRequest;
use crate::error::{AppError, AppResult};
use crate::events::emit_chat_event;
use crate::sidecar::proxy;
use crate::sidecar::sse::SseParser;

/// 流式空闲超时：连续 90s 无任何数据块视为流挂起（BE-m8）。
/// 与前端批次 7 看门狗（90s 空闲 + 600s 总量）数值对齐——后端先到先报
/// 真实原因，前端看门狗退化为纯 UI 兜底。
pub(super) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// 流式总量上限（BE-m8）：防慢速滴流无限占用后台任务。
pub(super) const STREAM_TOTAL_TIMEOUT: Duration = Duration::from_mins(10);

/// 流超时类型（BE-m8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamTimeout {
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
pub(super) async fn next_stream_chunk<S, T, E>(
    stream: &mut S,
    idle_deadline: &mut tokio::time::Instant,
    total_deadline: tokio::time::Instant,
) -> std::result::Result<Option<std::result::Result<T, E>>, StreamTimeout>
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
///
/// # Errors
///
/// 请求转发失败、网络中断或空闲/总量超时（BE-m8）时返回错误。
pub(super) async fn stream_chat(
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
