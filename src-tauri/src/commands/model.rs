//! Embedding 模型下载命令：查询下载状态 / 启动下载（Sidecar `/models/download*` 代理）
//! + 离线模型包导入（Sidecar `/models/import` 代理）。
//!
//! 背景：Embedding 改为 Sidecar 进程内 ONNX 推理后，模型文件不再由 Ollama 提供，
//! 而是由 Sidecar 从 HF 镜像下载（见 `python-sidecar/app/services/model_download_service.py`）。
//! 前端的模型卡片按固定间隔轮询状态接口渲染进度条与失败原因。
//!
//! 下载之外还有一条**离线通道**：内网 / 无外网部署的机器拿不到 HF 镜像，此时用
//! 另一台机器已下载好的 `models` 目录（或其 zip）导入模型文件（见
//! `python-sidecar/app/services/model_import_service.py`）。
//!
//! 线程模型：下载在 Sidecar 侧的后台任务中进行，`start_model_download` **立即返回**
//! 当前状态（不阻塞 UI）；进度通过 `model_download_status` 轮询获取。导入则相反，
//! 是同步等待的本地拷贝（最大模型 2.2GB），需一次拿到「缺哪个文件」的结论，故用
//! `proxy::IMPORT_POST_TIMEOUT`（1h）而非默认的 10 分钟。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::security;
use crate::sidecar::proxy;
use crate::AppState;

/// Sidecar 下载状态查询端点（GET，模型名走查询串）。
const SIDECAR_DOWNLOAD_STATUS_PATH: &str = "/models/download/status";
/// Sidecar 下载启动端点（POST，模型名走请求体）。
const SIDECAR_DOWNLOAD_START_PATH: &str = "/models/download";
/// Sidecar 离线模型包导入端点（POST，路径走请求体）。
const SIDECAR_IMPORT_PATH: &str = "/models/import";

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

/// 离线模型包导入结果（对齐 Sidecar `ModelImportResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ModelImportResult {
    /// 本次导入（或替换）的模型名。
    pub imported: Vec<String>,
    /// 包内已就绪、按幂等跳过的模型名。
    pub skipped: Vec<String>,
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

/// 校验并规范化离线包路径（安全红线：路径参数必须过 `path_guard`）。
///
/// 返回 canonical 后的绝对路径再交给 Sidecar：相对路径、符号链接在另一进程里会被
/// 二次解析成别的目标，校验与使用必须是同一个路径。
///
/// # Errors
///
/// 路径不存在 / 命中黑名单（`security::validate`），或路径不是有效 UTF-8 时返回错误。
fn safe_package_path(path: &str) -> AppResult<String> {
    let safe = security::validate(path)?;
    safe.to_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::UnsafePath("离线包路径不是有效的 UTF-8 字符串".to_string()))
}

/// 构造 `/models/import` 请求体（纯函数，便于单测对齐 Sidecar 字段名）。
fn import_body(path: &str) -> String {
    serde_json::json!({ "path": path }).to_string()
}

/// 导入离线模型包（zip 文件，或 `models` 目录 / 单个模型目录）。
///
/// 用途：内网 / 无外网部署的机器拿不到 HF 镜像时，用另一台已下载好模型的机器上的
/// `models` 目录（或其 zip）离线分发模型文件，避免「检索时没有对应的模型」。
///
/// # Errors
///
/// 路径未通过安全校验、Sidecar 未握手、请求失败，或包内容不合法（带错误码
/// EMB-V-001 内容不符合要求 / EMB-U-002 导入失败）时返回错误。
#[tauri::command]
#[specta::specta]
pub async fn import_model_package(
    state: State<'_, AppState>,
    path: String,
) -> Result<ModelImportResult, String> {
    import_model_package_inner(&state, &path)
        .await
        .map_err(|e| e.to_string())
}

/// 导入命令的纯逻辑入口（便于单元测试，不依赖 `tauri::State` 的构造）。
async fn import_model_package_inner(state: &AppState, path: &str) -> AppResult<ModelImportResult> {
    let canonical = safe_package_path(path)?;
    let (psk, seq) = sidecar_call_parts(state)?;
    // 用导入专用超时（1h，见 proxy::IMPORT_POST_TIMEOUT）：数 GB 的包放网络共享盘时，
    // 默认的 10 分钟会在拷贝途中触顶，界面报失败而 Sidecar 其实还在拷
    let resp = proxy::forward_post_with_timeout(
        SIDECAR_IMPORT_PATH,
        &import_body(&canonical),
        &psk,
        seq,
        proxy::IMPORT_POST_TIMEOUT,
    )
    .await?;
    parse_import_result(&resp)
}

/// 解析 Sidecar 导入响应（纯函数，便于单测）。
///
/// # Errors
///
/// JSON 结构不符合 `ModelImportResult` 时返回序列化错误。
fn parse_import_result(body: &str) -> AppResult<ModelImportResult> {
    Ok(serde_json::from_str(body)?)
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

    /// 请求体字段名与 Sidecar `ModelImportRequest` 一致（HTTP 层契约）。
    #[test]
    fn import_body_uses_sidecar_field_name() {
        let body = import_body("/tmp/offline.zip");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["path"], "/tmp/offline.zip");
    }

    /// 导入响应解析：导入与跳过明细均透传。
    #[test]
    fn parse_import_result_reads_both_lists() {
        let body = r#"{"imported": ["bge-large-zh-v1.5"], "skipped": ["bge-reranker-v2-m3"]}"#;
        let result = parse_import_result(body).unwrap();
        assert_eq!(result.imported, vec!["bge-large-zh-v1.5"]);
        assert_eq!(result.skipped, vec!["bge-reranker-v2-m3"]);
    }

    /// 畸形导入响应 → 序列化错误（不 panic）。
    #[test]
    fn parse_import_result_rejects_malformed_json() {
        assert!(matches!(
            parse_import_result("not-json"),
            Err(AppError::Serialize(_))
        ));
    }

    /// 安全红线：不存在的路径被 path_guard 拒绝（在触达 Sidecar 之前）。
    #[test]
    fn safe_package_path_rejects_missing() {
        let err = safe_package_path("/nonexistent/filemind-offline-package.zip").unwrap_err();
        assert!(matches!(err, AppError::UnsafePath(_)), "实际: {err:?}");
    }

    /// 黑名单路径被拒绝（即使存在）。
    #[test]
    fn safe_package_path_rejects_blocked() {
        assert!(safe_package_path("/System/filemind-offline-package.zip").is_err());
        assert!(safe_package_path("/etc/hosts").is_err());
    }

    /// 合法路径 → 返回 canonical 绝对路径（校验与使用同一个路径）。
    #[test]
    fn safe_package_path_returns_canonical() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::NamedTempFile::new()?;
        let raw = tmp.path().to_str().ok_or("non-UTF8 path")?;
        let canonical = safe_package_path(raw)?;
        assert_eq!(canonical, tmp.path().canonicalize()?.to_string_lossy());
        Ok(())
    }
}
