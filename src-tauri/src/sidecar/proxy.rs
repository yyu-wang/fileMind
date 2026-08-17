//! Sidecar HTTP 转发：Rust 作为唯一出口代理 Python 服务。

use crate::error::{AppError, AppResult};

const SIDECAR_BASE_URL: &str = "http://127.0.0.1:8765";

/// 转发 GET 请求到 Sidecar 并返回响应文本。
///
/// # Errors
///
/// 请求发送或响应读取失败时返回 `SidecarUnavailable`。
pub async fn forward_get(path: &str) -> AppResult<String> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let resp = reqwest::get(&url)
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
/// 请求发送或响应读取失败时返回 `SidecarUnavailable`。
pub async fn forward_post(path: &str, body: &str) -> AppResult<String> {
    let url = format!("{SIDECAR_BASE_URL}{path}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
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
