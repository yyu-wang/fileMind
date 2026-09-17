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
    cleanup_orphan_sidecar, resolve_dev_binary_path, CloudSidecarEnv, LocalLlmSidecarEnv,
    SidecarManager, SIDECAR_PORT,
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

/// 读取 `app_config` 并注入本地生成后端 env（T3b）。
///
/// Sidecar 不读 SQLite，本地生成走 Ollama 还是内置 llama.cpp 引擎只能这样送进去。
/// 读取失败时**不注入**（Sidecar 用其默认值 `ollama`），不因配置读取失败阻断启动。
///
/// ⚠️ env 在 Sidecar 进程启动时冻结：用户在设置页切换后端后，需要重启 Sidecar 才生效
/// （设置页切换动作由 T5 负责联动重启）；而「Ollama 不可用自动回落内置引擎」不依赖
/// 本 env——那一步由 Sidecar 内的探测判定，见 `inference_probe_service`。
pub fn configure_local_llm_env(manager: &mut SidecarManager, database: &Arc<Mutex<Database>>) {
    let resolved = {
        let Ok(db_guard) = database.lock() else {
            log::warn!("数据库锁中毒，本地生成后端配置未注入（Sidecar 用默认 ollama）");
            return;
        };
        ConfigRepo::get(db_guard.conn())
    };
    match resolved {
        Ok(config) => {
            log::info!(
                "本地生成后端注入: backend={}, model={}",
                config.local_llm_backend,
                config.local_llm_model
            );
            manager.set_local_llm_env(LocalLlmSidecarEnv {
                backend: config.local_llm_backend,
                model: config.local_llm_model,
            });
        }
        Err(e) => {
            log::warn!("本地生成后端配置读取失败，未注入（Sidecar 用默认 ollama）: {e}");
        }
    }
}

/// BE-M3：启动前清理上次异常退出残留的孤儿 Sidecar（`ppid==1` 且名字匹配）。
///
/// 防止旧进程占住 8765 端口导致本次探活命中旧进程、新 PSK 握手必败的死循环。
/// 单实例插件在 `run()` 才生效、晚于 Sidecar 启动，此清理以「只杀真孤儿」兜住该竞态：
/// 活实例的 Sidecar `ppid` 非孤，不会被误杀。
pub fn cleanup_orphans() {
    cleanup_orphan_sidecar(SIDECAR_PORT);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use filemind_lib::commands::config::AppConfig;

    /// 建一个已跑完迁移的临时数据库并包成 `Arc<Mutex<_>>`。
    fn test_db() -> (Arc<Mutex<Database>>, tempfile::NamedTempFile) {
        let tmp = tempfile::NamedTempFile::new().expect("临时文件创建失败");
        let db = Database::open(tmp.path()).expect("数据库打开失败");
        (Arc::new(Mutex::new(db)), tmp)
    }

    fn test_manager() -> SidecarManager {
        SidecarManager::new(PathBuf::from("/nonexistent/filemind-sidecar"))
    }

    /// 迁移默认值被注入：backend=ollama、model=注册表默认 GGUF 标识。
    #[test]
    fn configure_injects_migration_defaults() {
        let (db, _tmp) = test_db();
        let mut manager = test_manager();

        configure_local_llm_env(&mut manager, &db);

        let env = manager.local_llm_env().expect("应已注入本地生成后端 env");
        assert_eq!(env.backend, "ollama");
        assert_eq!(env.model, "qwen2.5-3b-instruct");
    }

    /// 用户改为内置后端后，注入值随之更新（Sidecar 据此拉起 llama.cpp 引擎）。
    #[test]
    fn configure_injects_user_choice() {
        let (db, _tmp) = test_db();
        {
            let guard = db.lock().expect("锁");
            let mut config = ConfigRepo::get(guard.conn()).expect("读取配置");
            config.local_llm_backend = "builtin".to_string();
            ConfigRepo::upsert(guard.conn(), &config).expect("写入配置");
        }
        let mut manager = test_manager();

        configure_local_llm_env(&mut manager, &db);

        let env = manager.local_llm_env().expect("应已注入");
        assert_eq!(env.backend, "builtin");
    }

    /// 配置缺失（字段为空串）时仍原样注入：Sidecar 侧按空值回落自身默认值，
    /// 不在此处猜测语义。
    #[test]
    fn configure_injects_empty_values_as_is() {
        let (db, _tmp) = test_db();
        {
            let guard = db.lock().expect("锁");
            let mut config: AppConfig = ConfigRepo::get(guard.conn()).expect("读取配置");
            config.local_llm_backend = String::new();
            ConfigRepo::upsert(guard.conn(), &config).expect("写入配置");
        }
        let mut manager = test_manager();

        configure_local_llm_env(&mut manager, &db);

        assert_eq!(manager.local_llm_env().expect("应已注入").backend, "");
    }
}
