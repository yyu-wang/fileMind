//! Sidecar HTTP 转发：Rust 作为唯一出口代理 Python 服务。
//!
//! 每个请求附带 HMAC-SHA256 签名（[`SIGNATURE_HEADER`]）+ 递增序号
//!（[`REQUEST_SEQ_HEADER`]），由 Sidecar 中间件验签 + 防重放。
//!
//! 安全映射：T-01（Sidecar 通信篡改）。

use std::sync::OnceLock;

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

/// 转发 GET 请求到 Sidecar 并返回响应文本。
///
/// # Errors
///
/// 签名计算、请求发送或响应读取失败时返回 `SidecarUnavailable`。
pub async fn forward_get(path: &str, psk: &[u8], seq: u64) -> AppResult<String> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let canonical = handshake::build_request_canonical("GET", path, "", seq);
    let signature = handshake::sign(psk, &canonical)?;

    let resp = client()
        .get(&url)
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 请求失败: {e}")))?;
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取响应失败: {e}")))?;
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

    let client = client();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header(handshake::SIGNATURE_HEADER, &signature)
        .header(handshake::REQUEST_SEQ_HEADER, seq.to_string())
        .body(body.to_string())
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
/// # Errors
///
/// 签名计算或请求发送失败时返回 `SidecarUnavailable`。
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
    Ok(resp)
}

/// 转发 POST /shutdown 请求到 Sidecar 并返回响应文本。
///
/// 与 [`forward_post`] 的区别：`/shutdown` 无 body，对应 canonical string
/// 中 body 段为空，与中间件验签逻辑保持一致。
///
/// # Errors
///
/// 签名计算、请求发送或响应读取失败时返回 `SidecarUnavailable`。
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
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取 shutdown 响应失败: {e}")))?;
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
}
