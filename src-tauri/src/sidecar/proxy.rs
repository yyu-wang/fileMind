use crate::error::{AppError, AppResult};

const SIDECAR_BASE_URL: &str = "http://127.0.0.1:8765";

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
