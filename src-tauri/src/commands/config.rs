use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AppConfig {
    pub data_directory: String,
    pub inference_mode: String,
    pub embedding_model: String,
    pub max_file_size_mb: u64,
    pub language: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        Self {
            data_directory: format!("{home}/.filemind"),
            inference_mode: "local".to_string(),
            embedding_model: "bge-large-zh-v1.5".to_string(),
            max_file_size_mb: 100,
            language: "zh-CN".to_string(),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_config() -> Result<AppConfig, String> {
    Ok(AppConfig::default())
}

#[tauri::command]
#[specta::specta]
pub async fn update_config(config: AppConfig) -> Result<AppConfig, String> {
    Ok(config)
}
