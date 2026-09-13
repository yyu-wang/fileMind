//! Sidecar 生命周期查询与手动重试命令（P1-1）。
//!
//! `get_sidecar_status`：前端启动时查询初始状态（事件可能早于页面监听建立）。
//! `retry_sidecar_start`：`failed` 状态下用户手动重试，复用已解析的二进制路径
//! 重新走后台引导流程，不重启整个应用。

use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::State;

use crate::sidecar::spawn_sidecar_bootstrap;
use crate::AppState;

/// Sidecar 生命周期快照（状态栏 / 启动流程查询）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SidecarStatusInfo {
    /// 状态：`starting` / `ready` / `failed`。
    pub status: String,
    /// 附加说明（`failed` 时含错误原因，其余为 `None`）。
    pub message: Option<String>,
    /// 历史累计重启次数（排障用）。
    // 重启次数不会超出 JS Number 精度（2^53-1），标注为 Number 导出
    #[specta(type = specta_typescript::Number)]
    pub restart_count: u64,
}

/// 查询当前 Sidecar 生命周期状态。
///
/// # Errors
///
/// `sidecar_status` Mutex 中毒时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn get_sidecar_status(state: State<'_, AppState>) -> Result<SidecarStatusInfo, String> {
    let status = state.sidecar_status.lock().map_err(|e| {
        log::error!("sidecar_status Mutex 中毒: {e}");
        "内部状态读取失败".to_string()
    })?;
    let restart_count = state.sidecar_restart_count.load(Ordering::SeqCst);
    Ok(SidecarStatusInfo {
        status: status.name().to_string(),
        message: status.message(),
        restart_count,
    })
}

/// 手动重试 Sidecar 启动（`failed` 状态下前端按钮触发）。
///
/// 复用 `AppState.sidecar_binary` 已解析的路径（后台引导在任何尝试前都会记录），
/// 直接重新走后台引导流程。若正在启动中则拒绝（防并发双开）。
///
/// # Errors
///
/// 状态锁中毒、或当前正处于 `starting` 时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn retry_sidecar_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let status = state.sidecar_status.lock().map_err(|e| {
            log::error!("sidecar_status Mutex 中毒: {e}");
            "内部状态读取失败".to_string()
        })?;
        if matches!(&*status, crate::SidecarStatus::Starting) {
            return Err("Sidecar 正在启动中，请稍候".to_string());
        }
    }
    let binary = state
        .sidecar_binary
        .lock()
        .map_err(|e| {
            log::error!("sidecar_binary Mutex 中毒: {e}");
            "内部状态读取失败".to_string()
        })?
        .clone();
    log::info!("用户手动重试 Sidecar 启动（binary 已记录）");
    spawn_sidecar_bootstrap(app, binary, None);
    Ok(())
}
