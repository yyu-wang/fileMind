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
/// # Errors
///
/// 签名计算、请求发送或响应读取失败时返回 `SidecarUnavailable`。
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
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("读取响应失败: {e}")))?;
    Ok(text)
}
