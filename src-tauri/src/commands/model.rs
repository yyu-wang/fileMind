//! Embedding 模型下载命令：查询下载状态 / 启动下载（Sidecar `/models/download*` 代理）。
//!
//! 背景：Embedding 改为 Sidecar 进程内 ONNX 推理后，模型文件不再由 Ollama 提供，
//! 而是由 Sidecar 从 HF 镜像下载（见 `python-sidecar/app/services/model_download_service.py`）。
//! 前端的模型卡片按固定间隔轮询状态接口渲染进度条与失败原因。
//!
//! 线程模型：下载在 Sidecar 侧的后台任务中进行，`start_model_download` **立即返回**
//! 当前状态（不阻塞 UI）；进度通过 `model_download_status` 轮询获取。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use crate::AppState;

/// Sidecar 下载状态查询端点（GET，模型名走查询串）。
const SIDECAR_DOWNLOAD_STATUS_PATH: &str = "/models/download/status";
/// Sidecar 下载启动端点（POST，模型名走请求体）。
const SIDECAR_DOWNLOAD_START_PATH: &str = "/models/download";

/// Embedding 模型下载状态（对齐 Sidecar `ModelDownloadStatusResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelDownloadStatus {
    /// 注册表模型名。
    pub model_name: String,
    /// 状态：`idle`（未开始）/ `downloading` / `ready` / `failed`。
    pub status: String,
    /// 当前使用的镜像地址；未开始为 `None`。
    pub mirror: Option<String>,
    /// 已尝试次数（含当前这次）。
    pub attempt: u32,
    /// 已下载字节数（口径为 ONNX 权重文件）。
    #[specta(type = specta_typescript::Number)]
    pub downloaded_bytes: u64,
    /// 全部文件总字节数；无法探测时为 `None`（前端显示不确定进度）。
    #[specta(type = Option<specta_typescript::Number>)]
    pub total_bytes: Option<u64>,
    /// 失败原因（`status=failed` 时非空）。
    pub error: Option<String>,
    /// 状态最后更新时间（ISO 8601）。
    pub updated_at: String,
}

/// 查询 Embedding 模型下载状态（Sidecar `/models/download/status` 代理）。
///
/// # Errors
///
/// Sidecar 未握手（PSK 为 `None`）、请求失败或响应解析失败时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn model_download_status(
    state: State<'_, AppState>,
    model_name: String,
) -> Result<ModelDownloadStatus, String> {
    model_download_status_inner(&state, &model_name)
        .await
        .map_err(|e| e.to_string())
}

/// 状态查询的纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
async fn model_download_status_inner(
    state: &AppState,
    model_name: &str,
) -> AppResult<ModelDownloadStatus> {
    let (psk, seq) = sidecar_call_parts(state)?;
    let path = status_path(model_name);
    let body = proxy::forward_get(&path, &psk, seq).await?;
    parse_status(&body)
}

/// 启动（或手动重试）Embedding 模型下载。
///
/// 立即返回当前状态（下载在 Sidecar 后台进行）；`failed` 后再次调用表示用户手动重试。
///
/// # Errors
///
/// Sidecar 未握手（PSK 为 `None`）、请求失败或响应解析失败时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn start_model_download(
    state: State<'_, AppState>,
    model_name: String,
) -> Result<ModelDownloadStatus, String> {
    start_model_download_inner(&state, &model_name)
        .await
        .map_err(|e| e.to_string())
}

/// 启动下载的纯逻辑入口（便于单元测试）。
async fn start_model_download_inner(
    state: &AppState,
    model_name: &str,
) -> AppResult<ModelDownloadStatus> {
    let (psk, seq) = sidecar_call_parts(state)?;
    let body = serde_json::json!({ "model_name": model_name }).to_string();
    let resp = proxy::forward_post(SIDECAR_DOWNLOAD_START_PATH, &body, &psk, seq).await?;
    parse_status(&resp)
}

/// 取侧车调用所需的 PSK 与请求序号（PSK 未握手时报错）。
fn sidecar_call_parts(state: &AppState) -> AppResult<(Vec<u8>, u64)> {
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok((psk, seq))
}

/// 拼状态查询路径：`/models/download/status?model_name=<name>`。
///
/// 模型名只可能来自注册表（`[A-Za-z0-9._-]`，见 Sidecar 侧 `MODEL_REGISTRY`），
/// 不含需要转义的字符，故直接拼接。
fn status_path(model_name: &str) -> String {
    format!("{SIDECAR_DOWNLOAD_STATUS_PATH}?model_name={model_name}")
}

/// 解析 Sidecar 下载状态响应（纯函数，便于单测）。
///
/// # Errors
///
/// JSON 结构不符合 `ModelDownloadStatus` 时返回序列化错误。
fn parse_status(body: &str) -> AppResult<ModelDownloadStatus> {
    Ok(serde_json::from_str(body)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn status_path_carries_model_name_as_query() {
        assert_eq!(
            status_path("bge-large-zh-v1.5"),
            "/models/download/status?model_name=bge-large-zh-v1.5"
        );
    }

    #[test]
    fn parse_status_downloading_with_progress() {
        let body = r#"{
            "model_name": "bge-large-zh-v1.5",
            "status": "downloading",
            "mirror": "https://hf-mirror.com",
            "attempt": 1,
            "downloaded_bytes": 1048576,
            "total_bytes": 327363707,
            "error": null,
            "updated_at": "2026-09-15T10:00:00"
        }"#;
        let status = parse_status(body).unwrap();
        assert_eq!(status.status, "downloading");
        assert_eq!(status.mirror.as_deref(), Some("https://hf-mirror.com"));
        assert_eq!(status.attempt, 1);
        assert_eq!(status.downloaded_bytes, 1_048_576);
        assert_eq!(status.total_bytes, Some(327_363_707));
        assert!(status.error.is_none());
    }

    #[test]
    fn parse_status_failed_with_error_and_unknown_total() {
        let body = r#"{
            "model_name": "bge-large-zh-v1.5",
            "status": "failed",
            "mirror": "https://hf-mirror.com",
            "attempt": 3,
            "downloaded_bytes": 0,
            "total_bytes": null,
            "error": "下载失败（已自动重试 3 次）",
            "updated_at": "2026-09-15T10:05:00"
        }"#;
        let status = parse_status(body).unwrap();
        assert_eq!(status.status, "failed");
        assert_eq!(status.attempt, 3);
        assert_eq!(status.total_bytes, None);
        assert!(status.error.unwrap().contains("3 次"));
    }

    #[test]
    fn parse_status_rejects_malformed_json() {
        assert!(parse_status(r#"{"status": 1}"#).is_err());
    }
}
