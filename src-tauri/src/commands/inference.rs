use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum InferenceMode {
    Local,
    Cloud,
}

#[tauri::command]
#[specta::specta]
pub async fn get_inference_mode() -> Result<InferenceMode, String> {
    Ok(InferenceMode::Local)
}

#[tauri::command]
#[specta::specta]
pub async fn set_inference_mode(
    mode: InferenceMode,
    source: String,
) -> Result<InferenceMode, String> {
    crate::security::mode_switch::validate_mode_switch("local", &format!("{mode:?}"), &source)
        .map_err(|e| e.to_string())?;
    Ok(mode)
}
