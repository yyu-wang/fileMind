//! Sidecar HTTP 转发：Rust 作为唯一出口代理 Python 服务。
//!
//! 每个请求附带 HMAC-SHA256 签名（[`SIGNATURE_HEADER`]）+ 递增序号
//!（[`REQUEST_SEQ_HEADER`]），由 Sidecar 中间件验签 + 防重放。
//!
//! 安全映射：T-01（Sidecar 通信篡改）。

use std::sync::OnceLock;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::security::handshake;

const SIDECAR_BASE_URL: &str = "http://127.0.0.1:8765";

/// 全局共享的 HTTP Client 单例。
///
/// `reqwest::Client` 内建连接池、DNS 缓存、TLS 会话复用与 HTTP keep-alive，
/// 单例化避免每请求新建 Client 丢失连接复用收益，同时减少 alloc 与 TCP 握手开销。
/// 由 `OnceLock` 保证多线程安全，仅首次访问时初始化一次。
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 返回全局共享 `reqwest::Client` 引用（首次访问时惰性初始化）。
fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

/// 探活/握手专用的短超时 Client 单例（BE-C4）。
///
/// 健康检查与握手均为「短请求-短响应」，3 秒总超时足够；超时保证 Sidecar
/// 挂死（接受连接但不响应）时调用方快速失败——watchdog 持锁调用 `health_check`，
/// 无超时会连看门狗一起挂死。流式转发必须继续用 [`client`]：总超时会掐断
/// SSE 长流，两者不可混用。
static PROBE_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 返回探活/握手专用 `reqwest::Client`（3 秒总超时）。
pub(crate) fn probe_client() -> &'static reqwest::Client {
    PROBE_CLIENT.get_or_init(|| {
        // build 失败仅理论上可能（TLS 后端初始化异常）；OnceLock 语义无法返回
        // Result，退回默认 Client 保证可用性优先，并留日志供排障
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_else(|e| {
                log::warn!("探活 Client 构建失败，退回无超时默认 Client: {e}");
                reqwest::Client::new()
            })
    })
}

/// 转发 GET 请求到 Sidecar 并返回响应文本。
///
/// # Errors
///
/// 签名计算、请求发送、响应读取失败，或 Sidecar 返回非 2xx 时返回 `SidecarUnavailable`。
pub async fn forward_get(path: &str, psk: &[u8], seq: u64) -> AppResult<String> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let canonical = handshake::build_request_canonical("GET", path, "", seq);
    let signature = handshake::sign(psk, &canonical)?;

    let resp = client()
        .get(&url)
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .timeout(Duration::from_secs(30)) // 30 秒超时，GET 请求应快速返回
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 请求失败: {e}")))?;
    // BE-m6：与 forward_post 对齐——非 2xx 的错误体形状与成功响应不同，
    // 直接返回会让调用方把错误体当成功响应反序列化（报误导性 missing field）
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取响应失败: {e}")))?;
    if !status.is_success() {
        return Err(sidecar_error_detail(status, &body));
    }
    Ok(body)
}

/// 转发 JSON POST 请求到 Sidecar 并返回响应文本。
///
/// Sidecar 返回非 2xx（如 503 Embedding 不可用）时，错误体为
/// `{"detail": "..."}`，与成功响应形状不同。此处直接以
/// [`AppError::SidecarUnavailable`] 返回提取后的真实错误信息，
/// 避免调用方把错误体当成功响应反序列化（报误导性的 missing field）。
///
/// # Errors
///
/// 签名计算、请求发送、响应读取失败，或 Sidecar 返回非 2xx 时返回 `SidecarUnavailable`。
pub async fn forward_post(path: &str, body: &str, psk: &[u8], seq: u64) -> AppResult<String> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let canonical = handshake::build_request_canonical("POST", path, body, seq);
    let signature = handshake::sign(psk, &canonical)?;

    let resp = client()
        .post(&url)
        .header("Content-Type", "application/json")
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .body(body.to_string())
        .timeout(Duration::from_mins(10)) // 10 分钟超时，防止索引构建等长任务挂起
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar POST 失败: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取响应失败: {e}")))?;
    if !status.is_success() {
        return Err(sidecar_error_detail(status, &text));
    }
    Ok(text)
}

/// 把 Sidecar 非 2xx 响应转换为可读错误。
///
/// `FastAPI` 错误体统一为 `{"detail": "..."}`，优先提取 `detail` 字段；
/// `detail` 缺失或响应体不是 JSON 时，截取响应体前 200 字符兜底（空体给占位提示）。
fn sidecar_error_detail(status: reqwest::StatusCode, body: &str) -> AppError {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_owned))
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "(空响应体)".to_string()
            } else {
                trimmed.chars().take(200).collect()
            }
        });
    AppError::SidecarUnavailable(format!("Sidecar 错误 ({status}): {detail}"))
}

/// 转发 JSON POST 请求到 Sidecar 并返回流式响应（供 SSE 逐块读取）。
///
/// 与 [`forward_post`] 构造签名与请求头一致，但返回原始 `reqwest::Response`，
/// 调用方通过 `.bytes_stream()` 迭代响应体并逐块喂给 `sse::SseParser`。
///
/// BE-C5：返回前检查状态码——Sidecar 401/500 时错误 JSON 不是 SSE 帧，
/// 解析器产不出任何帧，调用方会「静默成功」。非 2xx 在此转为
/// `SidecarUnavailable`（提取 `detail`），由上层发 error 事件兜底。
///
/// # Errors
///
/// 签名计算、请求发送失败，或 Sidecar 返回非 2xx 时返回 `SidecarUnavailable`。
pub async fn forward_post_stream(
    path: &str,
    body: &str,
    psk: &[u8],
    seq: u64,
) -> AppResult<reqwest::Response> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let canonical = handshake::build_request_canonical("POST", path, body, seq);
    let signature = handshake::sign(psk, &canonical)?;

    let resp = client()
        .post(&url)
        .header("Content-Type", "application/json")
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar POST 失败: {e}")))?;
    ensure_stream_success(resp).await
}

/// 校验流式响应状态码：非 2xx 读取 body 提取错误信息并转为 Err。
///
/// 独立成函数便于单测（`http::Response` 直接构造，不依赖网络）。
async fn ensure_stream_success(resp: reqwest::Response) -> AppResult<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取错误响应体失败: {e}")))?;
    Err(sidecar_error_detail(status, &text))
}

/// 转发 POST /shutdown 请求到 Sidecar 并返回响应文本。
///
/// 与 [`forward_post`] 的区别：`/shutdown` 无 body，对应 canonical string
/// 中 body 段为空，与中间件验签逻辑保持一致。
///
/// # Errors
///
/// 签名计算、请求发送、响应读取失败，或 Sidecar 返回非 2xx 时返回 `SidecarUnavailable`。
pub async fn forward_shutdown(psk: &[u8], seq: u64) -> AppResult<String> {
    const PATH: &str = "/shutdown";
    let url = format!("{SIDECAR_BASE_URL}{PATH}");
    // body 为空：与 HMAC 中间件空 body canonical 构造完全一致
    let canonical = handshake::build_request_canonical("POST", PATH, "", seq);
    let signature = handshake::sign(psk, &canonical)?;

    let resp = client()
        .post(&url)
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar shutdown 请求失败: {e}")))?;
    // BE-m6：与 forward_post 对齐——非 2xx（如验签失败 401）不能当成功文本返回，
    // 否则优雅关闭失败被静默吞掉、只能靠 Drop 兜底 hard kill
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取 shutdown 响应失败: {e}")))?;
    if !status.is_success() {
        return Err(sidecar_error_detail(status, &body));
    }
    Ok(body)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// `FastAPI` 标准错误体含 `detail` → 提取 `detail` 作为真实错误原因。
    #[test]
    fn sidecar_error_detail_extracts_detail_field() {
        let err = sidecar_error_detail(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            r#"{"detail":"建立索引失败: Embedding 模型未拉取"}"#,
        );
        assert!(
            err.to_string()
                .contains("建立索引失败: Embedding 模型未拉取"),
            "错误信息应包含 detail 原文，实际: {err}"
        );
    }

    /// 响应体不是 JSON 且缺少 detail → 截取原文兜底。
    #[test]
    fn sidecar_error_detail_falls_back_to_raw_body() {
        let err = sidecar_error_detail(reqwest::StatusCode::BAD_GATEWAY, "Bad Gateway");
        assert!(
            err.to_string().contains("Bad Gateway"),
            "非 JSON 错误体应原样呈现，实际: {err}"
        );
    }

    /// 空响应体 → 占位提示，不 panic。
    #[test]
    fn sidecar_error_detail_handles_empty_body() {
        let err = sidecar_error_detail(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "");
        assert!(
            err.to_string().contains("空响应体"),
            "空响应体应给出占位提示，实际: {err}"
        );
    }

    /// `reqwest::Response` 可由 `http::Response` 直接转换，无需真实网络。
    fn mock_response(status: u16, body: &str) -> reqwest::Response {
        http::Response::builder()
            .status(status)
            .body(body.to_string())
            .unwrap()
            .into()
    }

    /// BE-C5：非 2xx + `FastAPI` 错误体 → Err 提取 detail 与状态码。
    #[tokio::test]
    async fn stream_non_2xx_with_detail_becomes_error() {
        let resp = mock_response(401, r#"{"detail":"HMAC 验签失败"}"#);
        let err = ensure_stream_success(resp)
            .await
            .expect_err("401 应转为 Err");
        let msg = err.to_string();
        assert!(msg.contains("401"), "应包含状态码: {msg}");
        assert!(msg.contains("HMAC 验签失败"), "应提取 detail: {msg}");
    }

    /// 非 2xx + 非 JSON body → 截取原文兜底（不 panic、不空消息）。
    #[tokio::test]
    async fn stream_non_2xx_non_json_body_falls_back_to_text() {
        let resp = mock_response(503, "Service Unavailable");
        let err = ensure_stream_success(resp)
            .await
            .expect_err("503 应转为 Err");
        assert!(err.to_string().contains("503"));
        assert!(err.to_string().contains("Service Unavailable"));
    }

    /// 2xx 原样放行，body 未被消费（调用方继续 `bytes_stream`）。
    #[tokio::test]
    async fn stream_success_passes_through() {
        let resp = mock_response(200, "");
        let passed = ensure_stream_success(resp).await.expect("200 应放行");
        assert_eq!(passed.status(), reqwest::StatusCode::OK);
    }
}
