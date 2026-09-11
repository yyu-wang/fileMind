//! Sidecar 后台引导：把「启动 + 握手」移出 Tauri `setup` 关键路径，窗口秒开。
//!
//! 设计（P1-1）：
//! - `setup` 只做二进制路径解析（毫秒级）并调用 [`spawn_sidecar_bootstrap`] 立即返回
//! - 后台线程执行 `start_with_handshake`（打包态可能 30s+），完成后回写
//!   `AppState`（manager / PSK / binary / seq / status）
//! - 全程经 `sidecar-status` 事件推送状态：`starting` → `ready` / `failed`
//! - 失败不再 `process::exit`：前端展示错误 + 用户手动重试（`retry_sidecar_start`）

use std::path::{Path, PathBuf};

use tauri::{Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::events::types::SidecarStatusEvent;
use crate::security::log_redact;
use crate::sidecar::manager::{
    main_exe_in_dir, resolve_bundle_binary_path, CloudSidecarEnv, SidecarManager,
};
use crate::{AppState, SidecarStatus};

/// 启动 Sidecar 并完成握手（内部新建 current-thread tokio runtime）。
///
/// 供同步调用方在后台线程使用（`setup` / `main` 阶段没有异步上下文）。
///
/// # Errors
///
/// 与 [`SidecarManager::start_with_handshake`] 相同：任何启动或握手步骤失败。
fn start_with_handshake_blocking(manager: &mut SidecarManager) -> AppResult<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::SidecarUnavailable(format!("tokio runtime 初始化失败: {e}")))?;
    runtime.block_on(async { manager.start_with_handshake().await })
}

/// 更新 `AppState.sidecar_status` 并向前端发射 `sidecar-status` 事件。
///
/// Mutex 中毒（极罕见）时仅记录日志不中断调用方；前端可经
/// `get_sidecar_status` 命令在下次查询时对齐真值。
pub fn update_sidecar_status(app: &tauri::AppHandle, status: SidecarStatus) {
    let payload = SidecarStatusEvent {
        status: status.name().to_string(),
        message: status.message(),
    };
    let state = app.state::<AppState>();
    match state.sidecar_status.lock() {
        Ok(mut guard) => {
            *guard = status;
        }
        Err(e) => {
            log::error!("sidecar_status Mutex 中毒，状态更新失败: {e}");
        }
    }
    if let Err(e) = app.emit("sidecar-status", payload) {
        log::warn!("sidecar-status 事件发送失败: {e}");
    }
}

/// 按优先级挑选 Sidecar 二进制路径（纯逻辑，便于单测）：
/// `env` 强制覆盖 > 预解析的 dev 路径 > bundle 路径。
///
/// `env` 指定但路径不存在时不回落，直接返回错误——显式配置错误应暴露而非静默忽略。
///
/// `env` 语义是「直接指定要执行的 sidecar 可执行文件」：P2-2 起 onedir **目录**与
/// 可执行**文件**（CI E2E / dev 的 wrapper 脚本）都接受，与
/// [`crate::sidecar::manager::resolve_dev_binary_path`] 的优先级 1 保持一致。
fn pick_binary_path(
    env_override: Option<&str>,
    hint: &Path,
    bundle: Option<PathBuf>,
) -> AppResult<PathBuf> {
    if let Some(ov) = env_override.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(ov);
        if p.is_dir() {
            if let Some(exe) = main_exe_in_dir(&p) {
                return Ok(exe);
            }
        } else if p.is_file() {
            return Ok(p);
        }
        return Err(AppError::SidecarUnavailable(format!(
            "FILEMIND_SIDECAR_BINARY 指向的路径不存在（或 onedir 目录内无主可执行）: {ov}"
        )));
    }
    if !hint.as_os_str().is_empty() && hint.is_file() {
        return Ok(hint.to_path_buf());
    }
    bundle.filter(|p| p.is_file()).ok_or_else(|| {
        AppError::SidecarUnavailable("未找到 Sidecar 二进制（dev/bundle 均未命中）".into())
    })
}

/// 解析当前环境最终使用的 Sidecar 二进制路径。
///
/// bundle 路径解析需要 `AppHandle`，仅 macOS/Windows 生效（与 main 阶段规则一致）。
fn resolve_bootstrap_path(
    app: &tauri::AppHandle,
    binary_hint: PathBuf,
    env_override: Option<String>,
) -> AppResult<PathBuf> {
    let bundle = if cfg!(any(target_os = "macos", target_os = "windows")) {
        resolve_bundle_binary_path(app).ok()
    } else {
        None
    };
    pick_binary_path(env_override.as_deref(), &binary_hint, bundle)
}

/// 记录当前尝试的二进制路径到 `AppState.sidecar_binary`（成功/失败都记，供重试复用）。
fn set_binary_path(app: &tauri::AppHandle, path: PathBuf) {
    let state = app.state::<AppState>();
    // 先绑定再 match：直接 `match state.sidecar_binary.lock()` 时临时 Result 的
    // 析构顺序与 `state` 的生命周期冲突（E0597），绑定变量后声明序保证先析构
    let lock_result = state.sidecar_binary.lock();
    match lock_result {
        Ok(mut guard) => {
            *guard = path;
        }
        Err(e) => {
            log::error!("sidecar_binary Mutex 中毒，路径记录失败: {e}");
        }
    }
}

/// 读取当前 manager 的云端 env 配置（供新 manager 继承，T7.4）。
fn current_cloud_env(app: &tauri::AppHandle) -> Option<CloudSidecarEnv> {
    let state = app.state::<AppState>();
    state
        .sidecar_manager
        .lock()
        .ok()
        .and_then(|m| m.cloud_env().cloned())
}

/// 引导成功后替换 `AppState` 中的 manager / PSK / seq。
///
/// manager 锁中毒时兜底杀掉新进程，避免孤儿 Sidecar 占住端口（BE-M3）。
fn install_manager(app: &tauri::AppHandle, mut new_mgr: SidecarManager, new_psk: Vec<u8>) {
    let state = app.state::<AppState>();
    {
        let Ok(mut old_mgr) = state.sidecar_manager.lock() else {
            log::error!("sidecar_manager Mutex 中毒，引导结果未写入（AI 功能不可用）");
            let _ = new_mgr.stop_hard();
            return;
        };
        // 旧 manager 若已有进程（引导前不应有，防御性兜底）先杀避免端口泄漏
        let _ = old_mgr.stop_hard();
        *old_mgr = new_mgr;
    }
    if let Ok(mut psk_guard) = state.sidecar_psk.lock() {
        *psk_guard = Some(new_psk);
    } else {
        log::error!("sidecar_psk Mutex 中毒，PSK 未写入（代理请求将 401）");
    }
    state
        .request_seq
        .store(0, std::sync::atomic::Ordering::SeqCst);
}

/// 后台引导线程主体：解析路径 → 启动握手 → 回写 `AppState`。
///
/// 任何失败都收敛到 `Failed` 状态并推送事件，**不退出进程**（P1-1）。
fn bootstrap_worker(app: tauri::AppHandle, binary_hint: PathBuf, env_override: Option<String>) {
    update_sidecar_status(&app, SidecarStatus::Starting);

    let binary_path = match resolve_bootstrap_path(&app, binary_hint, env_override) {
        Ok(p) => p,
        Err(e) => {
            log::error!("Sidecar 引导失败（路径解析）: {e}");
            update_sidecar_status(&app, SidecarStatus::Failed(e.to_string()));
            return;
        }
    };
    log::info!(
        "Sidecar 引导使用路径: {}",
        log_redact::sanitize_path(&binary_path.display().to_string())
    );
    // 记录尝试路径：后续启动失败时 `retry_sidecar_start` 直接复用，无需重新解析
    set_binary_path(&app, binary_path.clone());

    let mut new_mgr = SidecarManager::new(binary_path);
    // 继承云端模式 env（代理地址/token/脱敏开关），重启后保持（T7.4）
    if let Some(cloud) = current_cloud_env(&app) {
        new_mgr.set_cloud_env(cloud);
    }

    match start_with_handshake_blocking(&mut new_mgr) {
        Ok(new_psk) => {
            install_manager(&app, new_mgr, new_psk);
            update_sidecar_status(&app, SidecarStatus::Ready);
            log::info!("Sidecar 引导完成（后台线程）");
        }
        Err(e) => {
            log::error!("Sidecar 引导失败（启动/握手）: {e}");
            update_sidecar_status(&app, SidecarStatus::Failed(e.to_string()));
        }
    }
}

/// 在后台线程启动 Sidecar 引导（`setup` 调用，不阻塞窗口显示）。
///
/// `binary_hint`：`main` 阶段预解析的 dev 路径（未命中时为空）。
/// `env_override`：`FILEMIND_SIDECAR_BINARY` 环境变量值（未设置时 `None`）。
pub fn spawn_sidecar_bootstrap(
    app: tauri::AppHandle,
    binary_hint: PathBuf,
    env_override: Option<String>,
) {
    let handle = app.clone();
    let spawn_result = std::thread::Builder::new()
        .name("sidecar-bootstrap".into())
        .spawn(move || bootstrap_worker(handle, binary_hint, env_override));
    if let Err(e) = spawn_result {
        log::error!("sidecar-bootstrap 线程创建失败: {e}");
        update_sidecar_status(&app, SidecarStatus::Failed("后台启动线程创建失败".into()));
    }
}

#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;
