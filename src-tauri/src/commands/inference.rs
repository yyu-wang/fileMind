//! 推理模式命令：查询与切换本地/云端推理模式。

use serde::{Deserialize, Serialize};

/// 推理模式。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum InferenceMode {
    /// 本地推理（Ollama）。
    Local,
    /// 云端推理（API 转发）。
    Cloud,
}

/// 获取当前推理模式。
///
/// # Errors
///
/// 模式读取失败时返回错误。
#[tauri::command]
#[specta::specta]
pub fn get_inference_mode() -> Result<InferenceMode, String> {
    log::debug!("读取当前推理模式");
    Ok(InferenceMode::Local)
}

/// 切换推理模式，切换前经过安全阀校验。
///
/// # Errors
///
/// 模式切换被安全策略拒绝或来源校验失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn set_inference_mode(mode: InferenceMode, source: String) -> Result<InferenceMode, String> {
    crate::security::mode_switch::validate_mode_switch("local", &format!("{mode:?}"), &source)
        .map_err(|e| e.to_string())?;
    Ok(mode)
}
