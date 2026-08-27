//! 应用配置命令：读写 FileMind 全局配置，包含云端知情同意书签署/撤回。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::ConfigRepo;
use crate::AppState;

/// 云端推理服务提供商。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub enum CloudProvider {
    /// `OpenAI`（gpt-4o 等）。
    Openai,
    /// `DeepSeek`。
    Deepseek,
}

/// 应用全局配置（持久化于 `SQLite` `app_config` 表）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AppConfig {
    /// 数据目录（数据库与索引存放位置）。
    pub data_directory: String,
    /// 推理模式：`local` 或 `cloud`（P1 仅这两种；`hybrid` 留待 T11）。
    pub inference_mode: String,
    /// 本地 Embedding 模型名称。
    pub embedding_model: String,
    /// 本地 LLM 推理模型名称（聊天/分类使用）。
    pub llm_model: String,
    /// 参与扫描的最大单文件大小（MB）。
    // specta-typescript 默认禁止 u64 导出；MB 单位下 u32 已足够，但保留 u64 语义
    #[specta(type = specta_typescript::Number)]
    pub max_file_size_mb: u64,
    /// 界面语言（BCP 47）。
    pub language: String,
    /// 是否已完成首次启动引导（0/1）。
    pub onboarding_completed: bool,
    /// 云端知情同意书是否已签署（0/1）。
    pub cloud_consent_signed: bool,
    /// 同意书版本号（已签署时为 `Some("v1.0")`，未签为 `None`）。
    pub cloud_consent_version: Option<String>,
    /// 云端提供商（已签署时为 `Some`，未签为 `None`）。
    pub cloud_consent_provider: Option<CloudProvider>,
    /// 签署时间（ISO 8601 字符串，未签为 `None`）。
    pub cloud_consent_signed_at: Option<String>,
    /// 云端推理模型名（如 `gpt-4o` / `deepseek-chat`；空串表示未指定，回落到 Provider 默认值）。
    #[serde(default)]
    pub cloud_model: String,
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
            llm_model: "qwen3.8-27b".to_string(),
            max_file_size_mb: 100,
            language: "zh-CN".to_string(),
            onboarding_completed: false,
            cloud_consent_signed: false,
            cloud_consent_version: None,
            cloud_consent_provider: None,
            cloud_consent_signed_at: None,
            cloud_model: String::new(),
        }
    }
}

/// 云端知情同意书签署结果。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ConsentResult {
    /// 是否签署成功。
    pub success: bool,
}

/// 云端知情同意书撤回结果（含自动切回 local 模式的联动信息）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RevokeConsentResult {
    /// 是否撤回成功。
    pub success: bool,
    /// 撤回后自动切换到的模式（固定为 `local`，04 API §2-3d 联动）。
    pub switched_to: String,
}

/// 读取当前应用配置。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let db = lock_db(&state.db)?;
    ConfigRepo::get(db.conn()).map_err(|e| format!("DB-C-001:配置读取失败 ({e})"))
}

/// 更新并持久化应用配置。
///
/// # Errors
///
/// 配置校验或持久化失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn update_config(state: State<'_, AppState>, config: AppConfig) -> Result<AppConfig, String> {
    log::info!(
        "配置更新: inference_mode={}, embedding_model={}, onboarding_completed={}",
        config.inference_mode,
        config.embedding_model,
        config.onboarding_completed
    );
    let db = lock_db(&state.db)?;
    ConfigRepo::upsert(db.conn(), &config).map_err(|e| format!("DB-C-002:配置持久化失败 ({e})"))?;
    drop(db);
    Ok(config)
}

/// 签署云端知情同意书。
///
/// 写入 `app_config` 表的 consent 字段并记录签署时间。
///
/// # Errors
///
/// 数据库写入失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn sign_cloud_consent(
    state: State<'_, AppState>,
    consent_version: String,
    provider: CloudProvider,
) -> Result<ConsentResult, String> {
    log::info!("签署云端知情同意书: version={consent_version}, provider={provider:?}");
    let db = lock_db(&state.db)?;
    ConfigRepo::sign_consent(db.conn(), &consent_version, provider)
        .map_err(|e| format!("DB-C-003:同意书签署失败 ({e})"))?;
    drop(db);
    Ok(ConsentResult { success: true })
}

/// 撤回云端知情同意书。
///
/// 清除 consent 字段并自动切回 `local` 模式（04 API §2-3d 撤回联动）。
/// 此切换是用户主动撤回同意触发的，不算违反「永不自动切换」约束。
///
/// # Errors
///
/// 数据库写入失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn revoke_cloud_consent(state: State<'_, AppState>) -> Result<RevokeConsentResult, String> {
    log::info!("撤回云端知情同意书，自动切回 local 模式");
    let db = lock_db(&state.db)?;
    ConfigRepo::revoke_consent(db.conn()).map_err(|e| format!("DB-C-004:同意书撤回失败 ({e})"))?;
    drop(db);
    Ok(RevokeConsentResult {
        success: true,
        switched_to: "local".to_string(),
    })
}

/// 锁定数据库句柄（统一错误码 DB-U-001）。
fn lock_db(
    db: &std::sync::Mutex<crate::db::Database>,
) -> Result<std::sync::MutexGuard<'_, crate::db::Database>, String> {
    db.lock().map_err(|e| {
        log::error!("DB lock poisoned: {e}");
        "DB-U-001:数据读取失败，请重启应用".to_string()
    })
}
