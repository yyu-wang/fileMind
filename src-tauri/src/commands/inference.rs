//! 推理模式命令：查询与切换本地/云端推理模式。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::ConfigRepo;
use crate::error::AppResult;
use crate::security::mode_switch::validate_mode_switch;
use crate::AppState;

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

/// 切换推理模式，切换前经过安全阀校验并持久化到 `app_config`。
///
/// # Errors
///
/// 模式切换被安全策略拒绝或持久化失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn set_inference_mode(
    state: State<'_, AppState>,
    mode: InferenceMode,
    source: String,
) -> Result<InferenceMode, String> {
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB-I-001:数据库读取失败，请重启应用 ({e})"))?;
    set_inference_mode_inner(db.conn(), mode, &source).map_err(|e| e.to_string())
}

/// 切换推理模式的纯逻辑（不依赖 `tauri::State`，便于单元测试）。
///
/// 从 `app_config` 读当前模式 → 安全阀校验（`validate_mode_switch`）→ 落库。
/// 目标模式统一转小写（`InferenceMode` 的 `Debug` 输出为大写），保证与
/// `validate_mode_switch` 的小写语义一致。Cloud 切换必须走同意书通道
/// （`has_consent()` 恒为 false），本命令仅放行手动切回本地等合法场景。
///
/// # Errors
///
/// 模式切换被安全阀拒绝或持久化失败时返回错误。
fn set_inference_mode_inner(
    conn: &Connection,
    mode: InferenceMode,
    source: &str,
) -> AppResult<InferenceMode> {
    let target = format!("{mode:?}").to_lowercase();
    let mut config = ConfigRepo::get(conn)?;
    let current = config.inference_mode.clone();
    validate_mode_switch(&current, &target, source)?;
    config.inference_mode.clone_from(&target);
    ConfigRepo::upsert(conn, &config)?;
    log::info!("推理模式切换: {current} -> {target} (source={source})");
    Ok(mode)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::db::Database;
    use crate::security::mode_switch::set_consent;
    use tempfile::NamedTempFile;

    /// 构建临时文件数据库并跑全部迁移。
    fn open_test_db() -> Database {
        let tmp = NamedTempFile::new().expect("临时文件创建失败");
        Database::open(tmp.path()).expect("数据库打开失败")
    }

    /// 切回本地：当前为 cloud（先签署同意书）→ 切回 local 应落库。
    #[test]
    fn switch_back_to_local_persists() {
        set_consent(false);
        let db = open_test_db();
        ConfigRepo::sign_consent(
            db.conn(),
            "v1.0",
            crate::commands::config::CloudProvider::Openai,
        )
        .unwrap();
        assert_eq!(ConfigRepo::get(db.conn()).unwrap().inference_mode, "cloud");

        let result = set_inference_mode_inner(db.conn(), InferenceMode::Local, "ui");
        assert!(result.is_ok());
        assert_eq!(ConfigRepo::get(db.conn()).unwrap().inference_mode, "local");
    }

    /// 切到云端：未同意时经安全阀拒绝，且不落库。
    #[test]
    fn switch_to_cloud_rejected_without_consent() {
        set_consent(false);
        let db = open_test_db();
        assert_eq!(ConfigRepo::get(db.conn()).unwrap().inference_mode, "local");

        let result = set_inference_mode_inner(db.conn(), InferenceMode::Cloud, "ui");
        assert!(result.is_err());
        assert_eq!(ConfigRepo::get(db.conn()).unwrap().inference_mode, "local");
    }

    /// 同模式切换：被安全阀拒绝（不产生冗余写）。
    #[test]
    fn switch_to_same_mode_rejected() {
        set_consent(false);
        let db = open_test_db();
        let result = set_inference_mode_inner(db.conn(), InferenceMode::Local, "ui");
        assert!(result.is_err());
        assert_eq!(ConfigRepo::get(db.conn()).unwrap().inference_mode, "local");
    }
}
