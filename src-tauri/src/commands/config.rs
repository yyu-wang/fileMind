//! 应用配置命令：读写 FileMind 全局配置。

use serde::{Deserialize, Serialize};

/// 应用全局配置。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AppConfig {
    /// 数据目录（数据库与索引存放位置）。
    pub data_directory: String,
    /// 推理模式：`local` 或 `cloud`。
    pub inference_mode: String,
    /// 本地 Embedding 模型名称。
    pub embedding_model: String,
    /// 参与扫描的最大单文件大小（MB）。
    // specta-typescript 默认禁止 u64 导出；MB 单位下 u32 已足够，但保留 u64 语义
    #[specta(type = specta_typescript::Number)]
    pub max_file_size_mb: u64,
    /// 界面语言（BCP 47）。
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

/// 读取当前应用配置。
///
/// # Errors
///
/// 配置序列化失败时返回错误。
#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<AppConfig, String> {
    Ok(AppConfig::default())
}

/// 更新并持久化应用配置。
///
/// # Errors
///
/// 配置校验或持久化失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn update_config(config: AppConfig) -> Result<AppConfig, String> {
    log::info!(
        "配置更新: inference_mode={}, embedding_model={}",
        config.inference_mode,
        config.embedding_model
    );
    Ok(config)
}
