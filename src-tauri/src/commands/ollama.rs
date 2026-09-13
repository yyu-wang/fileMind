//! Ollama 推理环境探测命令：可用性 + LLM 模型列表 + Embedding 模型可用性。
//!
//! 前端不能直连 Sidecar（HMAC+seq，PSK 在 Rust），本命令经 `proxy::forward_post`
//! 转发到 Sidecar `POST /inference/test` 并解析为结构化结果返回给前端
//! （设置页 / 引导页展示真实 Ollama 环境，T6.7）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use crate::AppState;

/// Sidecar 探测端点（API 规格书 §3.5，无请求参数，body 为空 JSON 对象）。
const SIDECAR_INFERENCE_TEST_PATH: &str = "/inference/test";
/// Sidecar Embedding 模型拉取端点。
const SIDECAR_INSTALL_MODEL_PATH: &str = "/inference/install-model";

/// 本地 Ollama 已安装的生成模型信息。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OllamaModelInfo {
    /// 模型名（已剥离 `:latest` 标签）。
    pub name: String,
    /// 模型文件大小（字节）。
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: u64,
    /// 模型家族（`details.family`，Ollama 侧可能缺失）。
    pub family: Option<String>,
    /// 最近修改时间（ISO 8601）。
    pub modified_at: Option<String>,
}

/// 单个 Embedding 模型在本地 Ollama 的可用性。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct EmbeddingModelAvailability {
    /// 注册表模型名（业务标识，非 Ollama 名）。
    pub name: String,
    /// 向量维度。
    pub dim: u32,
    /// 当前分配版本号。
    pub version: u32,
    /// 对应 Ollama 模型是否已安装。
    pub available: bool,
}

/// Ollama 推理环境探测结果（对齐 Sidecar `InferenceTestResponse`）。
///
/// Ollama 不可用时 `available=false` + `error_code='OLLAMA_UNAVAILABLE'`，
/// 探测本身不失败（HTTP 200），前端据此展示友好状态。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OllamaStatus {
    /// Ollama 服务是否可用。
    pub available: bool,
    /// 状态：`ok` | `unavailable`。
    pub status: String,
    /// 已安装的生成（LLM）模型列表。
    pub llm_models: Vec<OllamaModelInfo>,
    /// Embedding 模型可用性列表。
    pub embedding_models: Vec<EmbeddingModelAvailability>,
    /// 错误码（如 `OLLAMA_UNAVAILABLE`），可用时为 `None`。
    pub error_code: Option<String>,
    /// 人类可读的失败原因（展示用）。
    pub message: Option<String>,
}

/// 探测本地 Ollama 推理环境（Sidecar `/inference/test` 代理）。
///
/// # Errors
///
/// Sidecar 未握手（PSK 为 `None`）、请求失败或响应解析失败时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn ollama_status(state: State<'_, AppState>) -> Result<OllamaStatus, String> {
    ollama_status_inner(&state).await.map_err(|e| e.to_string())
}

/// 探测命令的纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
async fn ollama_status_inner(state: &AppState) -> AppResult<OllamaStatus> {
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let body = proxy::forward_post(SIDECAR_INFERENCE_TEST_PATH, "{}", &psk, seq).await?;
    parse_ollama_status(&body)
}

/// 解析 Sidecar `/inference/test` 响应 JSON（纯函数，便于单测）。
///
/// # Errors
///
/// JSON 结构不符合 `OllamaStatus` 时返回序列化错误。
fn parse_ollama_status(body: &str) -> AppResult<OllamaStatus> {
    Ok(serde_json::from_str(body)?)
}

/// Embedding 模型安装结果（对齐 Sidecar `ModelInstallResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelInstallResult {
    /// 安装是否成功。
    pub success: bool,
    /// 注册表模型名。
    pub model_name: String,
    /// Ollama 实际模型名。
    pub ollama_name: String,
    /// 结果消息。
    pub message: String,
}

/// 从 Ollama 拉取指定 Embedding 模型（Sidecar `/inference/install-model` 代理）。
///
/// # Errors
///
/// Sidecar 未握手（PSK 为 `None`）、请求失败或响应解析失败时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn install_embedding_model(
    state: State<'_, AppState>,
    model_name: String,
) -> Result<ModelInstallResult, String> {
    install_embedding_model_inner(&state, model_name)
        .await
        .map_err(|e| e.to_string())
}

/// 安装命令的纯逻辑入口（便于单元测试）。
async fn install_embedding_model_inner(
    state: &AppState,
    model_name: String,
) -> AppResult<ModelInstallResult> {
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let body = serde_json::json!({ "model_name": model_name }).to_string();
    let resp = proxy::forward_post(SIDECAR_INSTALL_MODEL_PATH, &body, &psk, seq).await?;
    parse_model_install_result(&resp)
}

/// 解析 Sidecar `/inference/install-model` 响应 JSON（纯函数，便于单测）。
///
/// # Errors
///
/// JSON 结构不符合 `ModelInstallResult` 时返回序列化错误。
fn parse_model_install_result(body: &str) -> AppResult<ModelInstallResult> {
    Ok(serde_json::from_str(body)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// Sidecar 响应解析：完整字段 + 可选字段 None。
    #[test]
    fn parse_full_status() {
        let body = r#"{
            "available": true,
            "status": "ok",
            "llm_models": [
                {"name": "qwen3.8-27b", "size_bytes": 16000000000, "family": "qwen3", "modified_at": "2026-08-01T00:00:00Z"}
            ],
            "embedding_models": [
                {"name": "bge-large-zh-v1.5", "dim": 1024, "version": 1, "available": true},
                {"name": "bge-m3", "dim": 1024, "version": 1, "available": false}
            ],
            "error_code": null,
            "message": null
        }"#;
        let status = parse_ollama_status(body).unwrap();
        assert!(status.available);
        assert_eq!(status.status, "ok");
        assert_eq!(status.llm_models.len(), 1);
        assert_eq!(status.llm_models[0].name, "qwen3.8-27b");
        assert_eq!(status.llm_models[0].size_bytes, 16_000_000_000);
        assert_eq!(status.llm_models[0].family.as_deref(), Some("qwen3"));
        assert_eq!(status.embedding_models.len(), 2);
        assert!(status.embedding_models[0].available);
        assert!(!status.embedding_models[1].available);
        assert_eq!(status.error_code, None);
        assert_eq!(status.message, None);
    }

    /// Ollama 不可用：available=false + `error_code`。
    #[test]
    fn parse_unavailable_status() {
        let body = r#"{
            "available": false,
            "status": "unavailable",
            "llm_models": [],
            "embedding_models": [],
            "error_code": "OLLAMA_UNAVAILABLE",
            "message": "Ollama 探测失败: connection refused"
        }"#;
        let status = parse_ollama_status(body).unwrap();
        assert!(!status.available);
        assert_eq!(status.status, "unavailable");
        assert!(status.llm_models.is_empty());
        assert_eq!(status.error_code.as_deref(), Some("OLLAMA_UNAVAILABLE"));
    }

    /// 畸形 JSON → 序列化错误（不 panic）。
    #[test]
    fn parse_malformed_returns_error() {
        let err = parse_ollama_status("not-json").unwrap_err();
        assert!(matches!(err, AppError::Serialize(_)));
    }
}
