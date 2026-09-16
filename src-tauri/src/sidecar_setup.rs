//! Sidecar 启动装配：二进制路径解析、云端代理 env 注入、残留孤儿进程清理。
//!
//! 本模块只做「启动前准备」，**不启动进程**：真正的启动 + HMAC 握手交给 `setup` 阶段的
//! 后台引导线程（`spawn_sidecar_bootstrap`），避免打包态冷启动 30s+ 阻塞窗口显示。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use filemind_lib::db::{ConfigRepo, Database};
use filemind_lib::security::cloud_proxy::{self, CLOUD_PROXY_HOST, CLOUD_PROXY_PORT};
use filemind_lib::security::{generate_token, log_redact};
use filemind_lib::sidecar::{
    cleanup_orphan_sidecar, resolve_dev_binary_path, CloudSidecarEnv, SidecarManager, SIDECAR_PORT,
};

use crate::startup::lock_db;

/// 解析 Sidecar 可执行文件路径，并回传 `FILEMIND_SIDECAR_BINARY` 的覆盖值（后台引导复用）。
///
/// 解析优先级：
///   1) `FILEMIND_SIDECAR_BINARY` env 强制指定（CI / 调试覆盖）
///   2) dev 布局解析（`CARGO_MANIFEST_DIR` / cwd 下的 repo `filemind/binaries/`）
///   3) macOS/Windows 打包态：dev 找不到 → **不退出**，留空由后台引导按 Tauri
///      `externalBin` 落地路径（主可执行文件同目录）启动，保证安装包在任意 cwd
///      （用户双击 / Spotlight 启动）下都能跑。
///
/// 其他平台无 bundle 兜底，dev 找不到直接退出（与旧行为一致）。
pub fn resolve_binary() -> (PathBuf, Option<String>) {
    let env_override = std::env::var("FILEMIND_SIDECAR_BINARY")
        .ok()
        .filter(|s| !s.is_empty());
    let can_bundle =
        cfg!(any(target_os = "macos", target_os = "windows")) && env_override.is_none();
    let sidecar_binary = match resolve_dev_binary_path(env_override.as_deref()) {
        Ok(p) => {
            log::info!(
                "Sidecar binary path: {}",
                log_redact::sanitize_path(&p.display().to_string())
            );
            p
        }
        Err(e) if can_bundle => {
            log::warn!("dev 模式未找到 Sidecar 二进制，交由后台引导按 bundle 路径解析: {e}");
            PathBuf::new()
        }
        Err(e) => {
            log::error!("Sidecar 二进制解析失败: {e}");
            std::process::exit(1);
        }
    };
    (sidecar_binary, env_override)
}

/// 装配云端代理（07-§4）并注入 Sidecar env，返回代理共享状态供 `setup` 启动代理。
///
/// 生成调用方共享 token → 按当前推理模式决定给 Sidecar 注入哪些云端 env（含脱敏开关）。
///
/// ⚠️ 端口绑定（`spawn_proxy_server`）**必须延迟到 Tauri `Builder` 构建之后**（即 setup
/// 回调内执行）：`tauri-plugin-single-instance` 的「唤醒已有实例并退出」逻辑只在
/// `builder.build()` 阶段才生效。若在 main 早期（插件生效前）抢先绑定 8766，当上一实例 /
/// 残留进程已占用该端口时，第二实例会在插件检测前因 `Address already in use` 直接闪退
/// （用户表现为「点击启动无任何页面」）。推迟后第二实例由单实例插件通知主实例 reveal
/// 主窗口并干净退出，主实例 setup 绑定失败才中止。
///
/// token 生成失败仍即退出（安全边界初始化不可跳过，模式与 `log_redact` 一致）。
pub fn configure_cloud_env(
    manager: &mut SidecarManager,
    database: &Arc<Mutex<Database>>,
) -> cloud_proxy::CloudProxyState {
    let proxy_token = match generate_token() {
        Ok(token) => token,
        Err(e) => {
            log::error!("云端代理 token 生成失败: {e}");
            std::process::exit(1);
        }
    };
    let proxy_state =
        cloud_proxy::CloudProxyState::new(proxy_token.clone()).with_db(Arc::clone(database));
    // 脱敏仅云端需要（本地 Ollama 需要原始内容做 RAG）；读失败按本地处理，不阻断启动
    // P-07：同时读出 active_cloud_provider（默认空串），用于 Sidecar env 注入
    let (masking_on, active_cloud_provider) = {
        let get_result = {
            let db_guard = lock_db(database);
            ConfigRepo::get(db_guard.conn())
        };
        match get_result {
            Ok(config) => (
                config.inference_mode == "cloud",
                config.active_cloud_provider.unwrap_or_default(),
            ),
            Err(_) => (false, String::new()),
        }
    };
    manager.set_cloud_env(CloudSidecarEnv {
        proxy_url: format!("http://{CLOUD_PROXY_HOST}:{CLOUD_PROXY_PORT}"),
        proxy_token,
        masking_on,
        active_cloud_provider,
    });
    proxy_state
}

/// BE-M3：启动前清理上次异常退出残留的孤儿 Sidecar（`ppid==1` 且名字匹配）。
///
/// 防止旧进程占住 8765 端口导致本次探活命中旧进程、新 PSK 握手必败的死循环。
/// 单实例插件在 `run()` 才生效、晚于 Sidecar 启动，此清理以「只杀真孤儿」兜住该竞态：
/// 活实例的 Sidecar `ppid` 非孤，不会被误杀。
pub fn cleanup_orphans() {
    cleanup_orphan_sidecar(SIDECAR_PORT);
}
