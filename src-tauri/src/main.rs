//! `FileMind` 桌面应用入口：初始化数据库、启动 Sidecar 与握手、注册 IPC 命令并启动 Tauri。
//!
//! 生命周期（含 Sidecar，T1.5 + T6.1 窗口/托盘管理）：
//! 1. 启动阶段：建 `SidecarManager` → `start_with_handshake` 成功 → 把 manager / PSK
//!    / seq / 重启计数放进 `AppState`
//! 2. 运行期：`setup` 中构建系统托盘 + spawn 后台 `watchdog`（独立 current-thread
//!    `tokio` runtime），每秒 tick：连续健康失败或 `Child::try_wait` 已退出 → 指数退避后
//!    `restart()`，并同步更新 `AppState` 里的 PSK + `reset` seq；1 分钟内 10 次重启 →
//!    `CrashLoop` 暂停，打 `error` 日志后 watchdog 自动退为「仅告警，不再自动恢复」
//! 3. 窗口关闭：`CloseRequested` 时检查 `IS_QUITTING` 标志：
//!    - false（默认）→ 阻止关闭 + `hide()` 最小化到托盘
//!    - true（托盘「退出」菜单置位）→ 走 `stop_graceful` sidecar 流程，再退出
//! 4. `Drop` 兜底：若 run 内部 panic 导致正常退出路径跳过，`Drop` 会用 `stopped` 标志位
//!    保证仅一次 hard kill，不重复杀进程
//!
//! 单实例：`tauri-plugin-single-instance` 防止二次启动产生孤儿 sidecar，第二次启动
//! 时回调里把已有主窗口显示出来并聚焦，新进程随后退出。

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use filemind_lib::commands;
use filemind_lib::db::{CategoryRepo, ConfigRepo, Database, OperationRepo};
use filemind_lib::error::AppError;
use filemind_lib::security::cloud_proxy::{self, CLOUD_PROXY_HOST, CLOUD_PROXY_PORT};
use filemind_lib::security::{generate_token, log_redact};
use filemind_lib::sidecar::{
    cleanup_orphan_sidecar, resolve_bundle_binary_path, resolve_dev_binary_path, CloudSidecarEnv,
    SidecarManager, WatchdogAction, SIDECAR_PORT,
};
use filemind_lib::tray::handle_tray_menu_event;
use filemind_lib::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    Emitter, Manager,
};

/// 健康看门狗基础轮询间隔（毫秒）。
///
/// 与 manager 内部 `HEALTH_POLL_INTERVAL_MS` 同值；集中到 main.rs 便于未来调优。
const WATCHDOG_TICK_MS: u64 = 1000;

fn get_db_path() -> PathBuf {
    // 数据目录优先读 FILEMIND_DATA_HOME（与 Python Sidecar 共用同一目录，保证
    // SQLite 与 LanceDB 落在同一根下）；未设置时回退到 ~/.filemind（生产默认位置）。
    let data_home = std::env::var("FILEMIND_DATA_HOME").unwrap_or_else(|_| {
        // BE-m9：HOME/USERPROFILE 均缺失时用 dirs 解析（unix 走 getpwuid 仍可得
        // 家目录）；彻底解析失败才退平台临时目录并显式告警——数据落临时目录
        // 有丢失风险，不再静默硬编码 /tmp。
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| {
                log::error!(
                    "无法解析用户家目录（HOME/USERPROFILE 均缺失且 dirs 解析失败），数据库回退平台临时目录，数据可能在清理时丢失"
                );
                std::env::temp_dir()
            });
        format!("{}/.filemind", home.display())
    });
    PathBuf::from(data_home).join("filemind.db")
}

/// 启动 Sidecar 并完成 HMAC 握手，返回 PSK。
///
/// 由 `block_on` 在当前线程 runtime 上执行（Tauri 初始化阶段没有异步上下文）。
///
/// # Errors
///
/// 任何启动或握手步骤失败时返回对应错误。
fn start_sidecar_with_handshake(manager: &mut SidecarManager) -> Result<Vec<u8>, AppError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::SidecarUnavailable(format!("tokio runtime 初始化失败: {e}")))?;
    runtime.block_on(async { manager.start_with_handshake().await })
}

/// 重启成功后同步新 PSK / seq / 重启计数到 `AppState`。
///
/// BE-M6：PSK 同步失败必须显式告警——此时实际进程用新 PSK 而 `AppState`
/// 还是旧值，所有代理请求将持续 401。
fn on_restart_success(state: &AppState, new_psk: Vec<u8>) {
    match state.sidecar_psk.lock() {
        Ok(mut psk_guard) => {
            *psk_guard = Some(new_psk);
        }
        Err(_) => {
            log::error!("重启后 PSK 同步 AppState 失败（Mutex 中毒），代理请求将持续 401");
        }
    }
    // 新 Sidecar 端 seq 从 0 开始，Rust 端必须跟随重置
    state.request_seq.store(0, Ordering::SeqCst);
    // 累计重启计数（排障用）
    let prev = state.sidecar_restart_count.fetch_add(1, Ordering::SeqCst);
    log::info!("Sidecar 重启成功，累计重启次数 = {}", prev + 1);
}

/// 在独立线程中启动 Sidecar 健康看门狗循环。
///
/// 用 `Arc<AppState>` 不现实（`State<T>` 由 Tauri 持有），故：
/// - 每次 tick 取 `AppHandle.state::<AppState>()` → lock `sidecar_manager`（时间尽可能短）
/// - `NeedRestart` 动作：release Mutex → sleep backoff → 重新 lock 调 restart
///   （避免持锁期间 `tokio::sleep` 阻塞其他线程访问 `AppState`）
///
/// 允许 `await_holding_lock`：`watchdog_tick().await` 持有 std Mutex 是预期
/// 行为 —— 当前 runtime 为 current-thread，await 期间不跨线程调度；且 manager
/// 持锁时间仅 /health 请求（几十毫秒级），不阻塞 Tauri UI。
#[allow(clippy::await_holding_lock)]
fn spawn_watchdog(app_handle: tauri::AppHandle) {
    let spawn_result = std::thread::Builder::new()
        .name("sidecar-watchdog".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("watchdog tokio runtime 初始化失败: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                loop {
                    // 阶段 A：取 AppState 短锁做 tick 决策
                    let action_result = {
                        let state = app_handle.state::<AppState>();
                        let Ok(mut manager) = state.sidecar_manager.lock() else {
                            log::error!("sidecar_manager Mutex 中毒，watchdog 退出");
                            return;
                        };
                        // 主路径已停止 → watchdog 干净退出
                        if manager.is_stopped() {
                            return;
                        }
                        manager.watchdog_tick().await
                    };

                    match action_result {
                        Ok(WatchdogAction::Idle) => {
                            // 健康：sleep 默认间隔再下一轮
                            tokio::time::sleep(Duration::from_millis(WATCHDOG_TICK_MS)).await;
                        }
                        Ok(WatchdogAction::NeedRestart) => {
                            // 先取 backoff（持锁时间短）
                            let backoff = {
                                let state = app_handle.state::<AppState>();
                                let Ok(manager) = state.sidecar_manager.lock() else {
                                    return;
                                };
                                manager.next_backoff()
                            };
                            log::warn!("Sidecar 需要重启，退避等待 {backoff:?} 后开始");
                            tokio::time::sleep(backoff).await;
                            // 阶段 B：重启（持锁，期间阻塞其他方访问 manager 可接受）
                            // 注：restart 路径用一次性 tokio runtime，避免主 runtime `rt`
                            // 已被 move 进外层 async 块无法再被借用到的借用错误。
                            let restart_result: Result<Vec<u8>, AppError> = {
                                let state = app_handle.state::<AppState>();
                                let Ok(mut manager) = state.sidecar_manager.lock() else {
                                    return;
                                };
                                let inner_rt = match tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                {
                                    Ok(rt) => rt,
                                    Err(e) => {
                                        log::error!("restart 阶段 tokio runtime 失败: {e}");
                                        return;
                                    }
                                };
                                inner_rt.block_on(manager.restart())
                            };
                            match restart_result {
                                Ok(new_psk) => {
                                    // 同步新 PSK / seq / 计数（失败告警见函数注释）
                                    on_restart_success(&app_handle.state::<AppState>(), new_psk);
                                }
                                Err(e) => {
                                    log::error!("Sidecar 重启失败: {e}");
                                    // 失败后仍继续循环（下一轮再次 NeedRestart 时退避更长），
                                    // 直到 CrashLoop 暂停。
                                }
                            }
                        }
                        Err(AppError::SidecarCrashLoop {
                            count,
                            window_secs,
                            message,
                        }) => {
                            log::error!(
                                "Sidecar 进入 CrashLoop（{count}/{window_secs}s）：{message}"
                            );
                            // BE-M7：通知前端展示「自动恢复已暂停」提示；
                            // 本分支自带 60s sleep，事件至多每分钟一条不会刷屏
                            if let Err(emit_err) = app_handle.emit(
                                "sidecar-crash-loop",
                                serde_json::json!({
                                    "count": count,
                                    "window_secs": window_secs,
                                    "message": message,
                                }),
                            ) {
                                log::warn!("sidecar-crash-loop 事件发送失败: {emit_err}");
                            }
                            // CrashLoop：每分钟只告警一次，避免刷日志
                            tokio::time::sleep(Duration::from_mins(1)).await;
                        }
                        Err(other) => {
                            log::warn!("watchdog tick 异常: {other}");
                            tokio::time::sleep(Duration::from_millis(WATCHDOG_TICK_MS)).await;
                        }
                    }
                }
            });
        });
    if let Err(e) = spawn_result {
        // 线程创建失败（极罕见，通常是系统资源耗尽）：打日志继续运行，
        // 缺少自动恢复 ≠ 主功能不可用
        log::error!("sidecar-watchdog 线程创建失败，跳过自动健康监控: {e}");
    }
}

/// 应用入口：初始化日志与数据库后启动 Sidecar 与握手，注册 IPC 命令后启动 Tauri 事件循环。
///
/// `generate_context!` 宏在编译期生成较大的上下文结构体（框架行为），
/// 栈占用为 Tauri 已知模式，非业务代码问题，定点豁免此 nursery lint。
/// 同时豁免 `too_many_lines`：主入口承担「解析路径 → 握手 → Tauri 构建 → setup → 事件」
/// 串联职责，硬拆会破坏可读性，已经按段落分层注释。
#[allow(clippy::large_stack_frames, clippy::too_many_lines)]
fn main() {
    // T7.1 日志脱敏：自定义 formatter 在「单一出口」统一脱敏，所有日志消息经
    // `log_redact::redact` 过滤后再输出（安全 I-03）。formatter 经静态路径调用
    // redact，与下方 init 顺序无关；正则未就绪时 redact 原样返回，仅可能在 init
    // 失败并退出前出现（失败日志为编译错误文本，不含敏感信息）。
    env_logger::Builder::new()
        .format(|buf, record| {
            let message = log_redact::redact(&record.args().to_string());
            writeln!(buf, "[{} {}] {}", record.level(), record.target(), message)
        })
        .parse_default_env()
        .init();

    // 正则集合编译：模式为编译期常量，正常不可达失败；此时日志可能明文泄漏
    // 敏感信息，直接以非零码退出，不进入无脱敏运行状态。
    if let Err(e) = log_redact::init() {
        log::error!("致命错误：日志脱敏正则初始化失败: {e}");
        std::process::exit(1);
    }
    let db_path = get_db_path();

    let database = match Database::open(&db_path) {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(e) => {
            log::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    // 工具闭包（局部作用域）：取 DB 守卫，main 启动阶段直接 `lock_db().conn()` 即可。
    let lock_db = || {
        database.lock().unwrap_or_else(|_| {
            log::error!("DB lock poisoned during startup");
            std::process::exit(1);
        })
    };

    // T6.5 内置分类种子：保证启发式分类有目标分类可用（幂等，失败不阻断启动）
    let seed_result = {
        let db_guard = lock_db();
        CategoryRepo::seed_builtin_categories(db_guard.conn())
    };
    match seed_result {
        Ok(0) => log::info!("内置分类已存在，跳过种子"),
        Ok(n) => log::info!("内置分类种子：新增 {n} 个分类"),
        Err(e) => log::warn!("内置分类种子失败（不影响启动）: {e}"),
    }

    // T9.5 E2E：`FILEMIND_E2E_SKIP_ONBOARDING=1` 时预置配置，让应用直达文件页。
    // 安全：仅 debug 构建生效；写入的是本次 E2E 的临时 SQLite（FILEMIND_DATA_HOME
    // 隔离），不影响真实用户配置；release 不编译此分支。
    #[cfg(debug_assertions)]
    if std::env::var("FILEMIND_E2E_SKIP_ONBOARDING").is_ok_and(|v| v == "1") {
        let mut config = {
            let db_guard = lock_db();
            ConfigRepo::get(db_guard.conn()).unwrap_or_default()
        };
        config.onboarding_completed = true;
        config.inference_mode = "local".to_string();
        if let Some(dir) = std::env::var("FILEMIND_E2E_DATA_DIR")
            .ok()
            .filter(|s| !s.is_empty())
        {
            config.data_directory = dir;
        }
        let upsert_result = {
            let db_guard = lock_db();
            ConfigRepo::upsert(db_guard.conn(), &config)
        };
        match upsert_result {
            Ok(()) => log::info!(
                "T9.5 E2E：FILEMIND_E2E_SKIP_ONBOARDING=1 已预置 onboarding_completed=true"
            ),
            Err(e) => log::error!("T9.5 E2E：预置配置失败: {e}"),
        }
    }

    // T3.5：启动时校验操作日志链式哈希完整性，检测到篡改仅告警、不阻断启动
    let verify_result = {
        let db_guard = lock_db();
        OperationRepo::verify_chain(db_guard.conn())
    };
    match verify_result {
        Ok(None) => log::info!("操作日志链式哈希校验通过"),
        Ok(Some(break_id)) => {
            log::error!("操作日志链式哈希校验失败，检测到篡改，断裂于记录 {break_id}");
        }
        Err(e) => log::error!("操作日志链式哈希校验出错: {e}"),
    }

    // ---- Sidecar 二进制解析与启动策略 ----
    //
    // 解析优先级：
    //   1) FILEMIND_SIDECAR_BINARY env 强制指定（CI / 调试覆盖）
    //   2) dev 布局解析（CARGO_MANIFEST_DIR / cwd 下的 repo `filemind/binaries/`）
    //   3) macOS/Windows 打包态：dev 找不到 → **不退出**，推迟到 setup 用 Tauri
    //      `externalBin` 落地路径（主可执行文件同目录）启动，保证安装包在任意
    //      cwd（用户双击 / Spotlight 启动）下都能跑。
    let env_override = std::env::var("FILEMIND_SIDECAR_BINARY")
        .ok()
        .filter(|s| !s.is_empty());
    let defer_to_bundle =
        cfg!(any(target_os = "macos", target_os = "windows")) && env_override.is_none();
    let (sidecar_binary, dev_started) = match resolve_dev_binary_path(env_override.as_deref()) {
        Ok(p) => {
            log::info!(
                "Sidecar binary path: {}",
                log_redact::sanitize_path(&p.display().to_string())
            );
            (p, true)
        }
        Err(e) if defer_to_bundle => {
            log::warn!("dev 模式未找到 Sidecar 二进制，推迟到 setup 按 bundle 路径启动: {e}");
            (PathBuf::new(), false)
        }
        Err(e) => {
            log::error!("Sidecar 二进制解析失败: {e}");
            std::process::exit(1);
        }
    };
    let mut sidecar_manager = SidecarManager::new(sidecar_binary.clone());

    // T7.4 云端代理（07-§4）：生成调用方共享 token → 启动本机代理（127.0.0.1:8766）→
    // 按当前推理模式决定给 Sidecar 注入哪些云端 env（含脱敏开关）。
    // 失败即退出（安全边界初始化不可跳过，模式与 log_redact 一致）。
    let proxy_token = match generate_token() {
        Ok(token) => token,
        Err(e) => {
            log::error!("云端代理 token 生成失败: {e}");
            std::process::exit(1);
        }
    };
    let proxy_state =
        cloud_proxy::CloudProxyState::new(proxy_token.clone()).with_db(Arc::clone(&database));
    if let Err(e) = cloud_proxy::spawn_proxy_server(proxy_state) {
        log::error!("云端代理启动失败: {e}");
        std::process::exit(1);
    }
    // 脱敏仅云端需要（本地 Ollama 需要原始内容做 RAG）；读失败按本地处理，不阻断启动
    // P-07：同时读出 active_cloud_provider（默认空串），用于 Sidecar env 注入
    let (masking_on, active_cloud_provider) = {
        let get_result = {
            let db_guard = lock_db();
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
    log::info!(
        "云端代理就绪: {CLOUD_PROXY_HOST}:{CLOUD_PROXY_PORT}, masking={masking_on}, active_provider={active_cloud_provider}"
    );
    sidecar_manager.set_cloud_env(CloudSidecarEnv {
        proxy_url: format!("http://{CLOUD_PROXY_HOST}:{CLOUD_PROXY_PORT}"),
        proxy_token,
        masking_on,
        active_cloud_provider,
    });

    // BE-M3：启动前清理上次异常退出残留的孤儿 Sidecar（ppid==1 且名字匹配），
    // 防止旧进程占住 8765 端口导致本次探活命中旧进程、新 PSK 握手必败死循环。
    // 单实例插件在 run() 才生效、晚于 Sidecar 启动，此清理以「只杀真孤儿」
    // 兜住该竞态：活实例的 Sidecar ppid 非孤，不会被误杀。
    cleanup_orphan_sidecar(SIDECAR_PORT);

    // dev 模式：本阶段直接启动 + 握手（失败退出，避免未验证身份进入主循环）。
    // 打包态 defer：本阶段不起进程，由 setup 用 bundle 路径完成启动与握手。
    let sidecar_psk: Option<Vec<u8>> = if dev_started {
        match start_sidecar_with_handshake(&mut sidecar_manager) {
            Ok(psk) => {
                // 主流程 manager 当前已持有 PSK（start_with_handshake 内部已存进
                // self.psk），若与 AppState 写入的 psk 不一致以 AppState 为准，
                // 这里同步拷贝一次保持一致。
                debug_assert!(sidecar_manager
                    .psk()
                    .is_none_or(|inner| Some(inner) == Some(psk.as_slice())));
                Some(psk)
            }
            Err(e) => {
                log::error!("Sidecar handshake failed: {e}");
                // BE-M3：std::process::exit 跳过 Drop，必须显式杀掉子进程再退出，
                // 否则留下孤儿进程占住端口（start_with_handshake 内部已清理一次，
                // 这里对「错误发生在 start 之前」等残余路径再兜底）。
                if let Err(kill_err) = sidecar_manager.stop_hard() {
                    log::error!("退出前清理 Sidecar 子进程失败: {kill_err}");
                }
                std::process::exit(1);
            }
        }
    } else {
        log::info!("Sidecar 已延迟到 setup 启动（bundle 路径）");
        None
    };

    // Tauri AppState 生命周期贯穿整个 Tauri 运行期，
    // 同时 main 栈变量也持有 AppState 引用直到 run() 返回；
    // 为了让 run() 返回后还能访问 manager/PSK/seq 做优雅关闭，这里 clone AppHandle：
    // 通过 AppHandle 就能从 managed state 再取出。
    let builder = tauri::Builder::default()
        // T6.1 单实例：第二个进程启动时回调里把已有窗口显示出来并聚焦，新进程随后退出
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // 自动更新：手动检查（设置页触发）；更新安装后经 process 插件重启
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    // T9.5 E2E：嵌入式 WebDriver server 仅 debug 构建注册（release 不携带自动化入口）。
    // 由 @wdio/tauri-service 以 driverProvider:'embedded' 连接 127.0.0.1:4445。
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    let builder = builder
        .manage(AppState {
            db: Arc::clone(&database),
            sidecar_manager: Mutex::new(sidecar_manager),
            sidecar_psk: Mutex::new(sidecar_psk),
            sidecar_binary: Mutex::new(sidecar_binary),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
        })
        .invoke_handler(tauri::generate_handler![
            // T6.6 RAG 问答（Sidecar /chat/stream SSE 代理）
            commands::chat::chat_stream,
            // T7.x 建立文件索引（Sidecar /index/build 代理）
            commands::index::build_index,
            // T6.5 智能分类预览（规则引擎 + 启发式）
            commands::classify::classify_preview,
            // T6.8 规则编辑（CRUD + 拖拽排序）
            commands::rules::list_rules,
            commands::rules::upsert_rule,
            commands::rules::delete_rule,
            commands::rules::reorder_rules,
            commands::rules::list_categories,
            commands::file_ops::scan_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::delete_files,
            commands::file_ops::undo_batch,
            commands::file_preview::read_file_preview,
            commands::file_query::list_files,
            commands::file_query::list_all_files,
            commands::file_query::search_files,
            commands::file_query::search_by_filename,
            commands::file_query::get_file_stats,
            commands::file_query::update_file_category,
            commands::file_query::get_operation_history,
            commands::file_query::get_batch_detail,
            commands::inference::get_inference_mode,
            commands::inference::set_inference_mode,
            commands::ollama::ollama_status,
            commands::ollama::install_embedding_model,
            commands::config::get_config,
            commands::config::update_config,
            commands::config::sign_cloud_consent,
            commands::config::revoke_cloud_consent,
            // T7.3 云端 API Key 存取（只存 Keychain、不回传完整 Key）
            commands::api_key::get_api_key_status,
            commands::api_key::set_api_key,
            commands::api_key::delete_api_key,
            // P-07 自定义云提供商管理（对应 cloud_providers 表 CRUD）
            commands::cloud_providers::list_cloud_providers,
            commands::cloud_providers::upsert_cloud_provider,
            commands::cloud_providers::delete_cloud_provider,
            // T9.5 E2E 测试专用命令（仅 debug 注册；release 不携带自动化入口）
            #[cfg(debug_assertions)]
            commands::e2e::e2e_get_test_dir,
        ])
        .setup(move |app| {
            // T1.3-P2：setup 内 AppHandle 可用 → 决策是否启用 bundle 路径覆盖
            //
            // 规则矩阵（main dev 解析先用，这里可能替换）：
            //   FILEMIND_SIDECAR_BINARY env 已设置             → 不动，强制 env 路径
            //   env 未设置 + 命中 macOS/Windows bundle / force → 走 Tauri resource_dir
            //   env 未设置 + dev cargo run                      → 保持 dev 解析结果
            let env_override_set = std::env::var("FILEMIND_SIDECAR_BINARY")
                .ok()
                .is_some_and(|s| !s.is_empty());
            let force_bundle = std::env::var("FILEMIND_FORCE_BUNDLE_PATH").is_ok();
            let native_bundle =
                cfg!(any(target_os = "macos", target_os = "windows"));
            let should_try_bundle = !env_override_set && (force_bundle || native_bundle);

            if should_try_bundle {
                match resolve_bundle_binary_path(app.handle()) {
                    Ok(bundle_path) => {
                        log::info!(
                            "命中 bundle Sidecar 路径: {}; 将替换当前 dev 路径并重新握手",
                            bundle_path.display()
                        );
                        let mut new_mgr = SidecarManager::new(bundle_path.clone());
                        // T7.4：bundle manager 必须继承 main 阶段解析好的云端 env
                        // （代理地址/token/脱敏开关/激活提供商），否则打包态云端模式
                        // 启动后 Sidecar 拿不到代理配置 → 云端请求直连外网（违规）。
                        {
                            let state = app.state::<AppState>();
                            let cloud = state
                                .sidecar_manager
                                .lock()
                                .ok()
                                .and_then(|m| m.cloud_env().cloned());
                            if let Some(cloud) = cloud {
                                new_mgr.set_cloud_env(cloud);
                            }
                        }
                        match start_sidecar_with_handshake(&mut new_mgr) {
                            Ok(new_psk) => {
                                let state = app.state::<AppState>();
                                // 顺序：先锁旧 manager → 调用 stop_hard 占位对象（dev 路径
                                // 的 manager 其实是真启动，务必杀避免端口/孤儿泄漏）→ 再 replace
                                {
                                    let Ok(mut old_mgr) = state.sidecar_manager.lock() else {
                                        log::error!("setup 替换 manager 时 Mutex 中毒，放弃 bundle 切换（沿用 dev 路径）");
                                        spawn_watchdog(app.handle().clone());
                                        return Ok(());
                                    };
                                    // 旧 manager 可能已经在握手后启动（真正持有 Child），
                                    // stop_hard 兜底确保旧进程一定杀掉。
                                    let _ = old_mgr.stop_hard();
                                    *old_mgr = new_mgr;
                                }
                                {
                                    let Ok(mut binary_guard) = state.sidecar_binary.lock() else {
                                        log::error!("setup 替换 sidecar_binary 时 Mutex 中毒");
                                        spawn_watchdog(app.handle().clone());
                                        return Ok(());
                                    };
                                    *binary_guard = bundle_path;
                                }
                                {
                                    let Ok(mut psk_guard) = state.sidecar_psk.lock() else {
                                        log::error!("setup 替换 sidecar_psk 时 Mutex 中毒");
                                        spawn_watchdog(app.handle().clone());
                                        return Ok(());
                                    };
                                    *psk_guard = Some(new_psk);
                                }
                                state.request_seq.store(0, Ordering::SeqCst);
                                log::info!("Sidecar 已切换为 bundle 路径并重新握手成功");
                            }
                            Err(e) => {
                                // 打包态（无 dev 进程）bundle 启动失败 = 核心引擎不可用，
                                // 直接中止启动，避免半可用应用；dev 场景回退沿用 dev 路径。
                                let had_dev = app
                                    .state::<AppState>()
                                    .sidecar_manager
                                    .lock()
                                    .is_ok_and(|m| m.psk().is_some());
                                if !had_dev {
                                    log::error!(
                                        "bundle Sidecar 启动+握手失败且无 dev 回退，应用中止: {e}"
                                    );
                                    return Err(e.into());
                                }
                                log::warn!("bundle manager 启动+握手失败，回退沿用 dev 路径（已可用）: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        // 打包态（无 dev 进程）且 bundle 未命中 = 安装包缺 Sidecar，
                        // 中止启动；dev 场景未命中属正常，保持 dev 路径。
                        let had_dev = app
                            .state::<AppState>()
                            .sidecar_manager
                            .lock()
                            .is_ok_and(|m| m.psk().is_some());
                        if !had_dev {
                            log::error!(
                                "未命中 bundle Sidecar 路径且无 dev 回退，应用中止: {e}"
                            );
                            return Err(e.into());
                        }
                        log::warn!("未命中 bundle Sidecar 路径（可能是 dev 环境），保持 dev 路径: {e}");
                    }
                }
            }
            spawn_watchdog(app.handle().clone());

            // T9.5 E2E：`FILEMIND_E2E=1` 时强制显示主窗口（debug 构建专用，避免依赖
            // 前端 main.tsx 的 show() 成功；release 不编译此分支）。
            #[cfg(debug_assertions)]
            if std::env::var("FILEMIND_E2E").is_ok_and(|v| v == "1") {
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(e) = window.show() {
                        log::warn!("T9.5 E2E：强制显示主窗口失败: {e}");
                    } else {
                        log::info!("T9.5 E2E：FILEMIND_E2E=1 已强制显示主窗口");
                    }
                }
            }

            // T6.1 系统托盘：菜单「显示主窗口 / 退出」+ 左键点击显示窗口
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .ok_or("default window icon not found")?;
            let _tray = tauri::tray::TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("FileMind")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| handle_tray_menu_event(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, _event| {
                    // 点击托盘图标时显示主窗口（macOS 上 on_menu_event 的 show 已覆盖左键点击；
                    // 这里兜底 Windows/Linux 行为）
                    let app = tray.app_handle();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                })
                .build(app)?;
            Ok(())
        });

    // run() 内部执行 Tauri 事件循环；返回 `Ok(())` 或 Err 都表明应用已经退出。
    // 为了确保 run 返回后仍能拿到 AppState（managed state 需要 AppHandle），
    // 把 AppHandle 通过 setup 闭包 clone 到外面的 Cell 里做不到（setup 是 FnOnce 已 move）。
    // 妥协方案：由于 AppState 作为 managed state 与 main 函数同生命周期，
    // `run` 返回后 Tauri 还没释放（当前栈未析构）。用一个 OnceLock AppHandle 引用：
    // 在 setup 中把 handle 写到一个 static OnceLock 里 —— 不方便；更简单做法：
    // 在 .setup(...) 注册的 setup 回调里通过 app.handle().clone() 让 watchdog 持有，
    // 同时把 AppHandle 也传给一个即将 `on_drop` 的局部结构（会引入 boilerplate）。
    //
    // 选定方案：**双保险**：
    // - run() 返回后，AppState 会作为 managed state（Tauri 2.x 释放顺序：先析构 Builder
    //   内的 state，再返回）；这时已经拿不到 AppHandle.state()。
    //   改为：在 setup 内部，额外注册一个 *Window CloseRequested* 监听，在窗口关闭时
    //   立即先执行 stop_graceful（在 run() 返回之前，Tauri state 仍存活）。
    // - run() 返回后的路径作为后备，仅打一行日志（无法直接访问 state 了）。
    //
    // 注：Tauri v2 提供 `on_window_event` 链式 API。

    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            // T6.1：根据 IS_QUITTING 区分「最小化到托盘」与「真退出」
            if filemind_lib::tray::IS_QUITTING.load(Ordering::SeqCst) {
                // 真退出路径：原 stop_graceful sidecar 流程
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let seq = state.request_seq.fetch_add(1, Ordering::SeqCst);
                let Ok(mut manager) = state.sidecar_manager.lock() else {
                    log::error!("sidecar_manager Mutex 中毒，无法优雅关 Sidecar");
                    return;
                };
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("CloseRequested tokio runtime 初始化失败: {e}");
                        let _ = manager.stop_hard();
                        return;
                    }
                };
                if let Err(e) = rt.block_on(manager.stop_graceful(seq)) {
                    log::error!("Sidecar 优雅关闭失败（已 fallback hard kill 兜底）: {e}");
                }
            } else {
                // 最小化到托盘：阻止默认关闭，仅隐藏窗口
                api.prevent_close();
                let _ = window.hide();
            }
        }
    });

    let run_result = builder.run(tauri::generate_context!());

    if let Err(e) = run_result {
        log::error!("Error while running tauri application: {e}");
        std::process::exit(1);
    }

    // 后备：若主窗口关闭事件路径未触发（极少，仅 headless/菜单退出等非 CloseRequested），
    // SidecarManager 此时由 Tauri managed state 析构 → Drop → stop_hard() 兜底杀一次。
    // 由于 stopped 标志位在 CloseRequested 主路径已经 set，Drop 不会重复 kill。
}
