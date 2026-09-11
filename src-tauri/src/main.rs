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
use filemind_lib::db::{CategoryRepo, ConfigRepo, Database, OperationRepo, RuleRepo};
use filemind_lib::error::AppError;
use filemind_lib::security::cloud_proxy::{self, CLOUD_PROXY_HOST, CLOUD_PROXY_PORT};
use filemind_lib::security::{generate_token, log_redact};
use filemind_lib::sidecar::{
    cleanup_orphan_sidecar, resolve_dev_binary_path, spawn_sidecar_bootstrap,
    update_sidecar_status, CloudSidecarEnv, SidecarManager, WatchdogAction, SIDECAR_PORT,
};
use filemind_lib::tray::{handle_tray_menu_event, reveal_main_window};
use filemind_lib::{AppState, SidecarStatus};
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
                            // 注：直接在外层 runtime 上 await——嵌套 `block_on`
                            // （旧实现：此处新建 inner_rt 并 block_on）会触发 tokio
                            // panic「Cannot start a runtime from within a runtime」，
                            // 且 release profile 为 panic=abort，整进程直接闪退
                            // （回归：打包版首次提问时 sidecar 加载 rerank 模型
                            // 饥饿事件循环 → /health 连续失败 → watchdog 重启 → 崩溃）。
                            let restart_result: Result<Vec<u8>, AppError> = {
                                let state = app_handle.state::<AppState>();
                                let Ok(mut manager) = state.sidecar_manager.lock() else {
                                    return;
                                };
                                manager.restart().await
                            };
                            match restart_result {
                                Ok(new_psk) => {
                                    // 同步新 PSK / seq / 计数（失败告警见函数注释）
                                    on_restart_success(&app_handle.state::<AppState>(), new_psk);
                                    // P1-1：重启成功同步前端状态（事件 + AppState）
                                    update_sidecar_status(&app_handle, SidecarStatus::Ready);
                                }
                                Err(e) => {
                                    log::error!("Sidecar 重启失败: {e}");
                                    // P1-1：单次重启失败即转 Failed 交给用户重试，
                                    // 不再无限退避重试（重启后 process/psk 均为 None，
                                    // watchdog 守卫会停在 Idle，见 manager.rs）
                                    update_sidecar_status(
                                        &app_handle,
                                        SidecarStatus::Failed(e.to_string()),
                                    );
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
                            // P1-1：同步为 Failed，前端展示错误 + 重试入口
                            update_sidecar_status(
                                &app_handle,
                                SidecarStatus::Failed(message.clone()),
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

/// 启动期窗口自愈守卫（缓解 macOS 26 + tao 0.35.x 上游间歇缺陷：
/// 窗口按 `visible: true` 创建但 `show` 偶发被系统静默吞掉，导致冷启动后
/// 进程/WebView 正常却无可见窗口，见 tauri#15517 / open-pdf-studio#208）。
///
/// 规则：仅当主窗口**从启动起从未可见**时才反复 `reveal`（最多约 8 次 / 20s）；
/// 一旦观察到窗口可见过即退出，之后用户「关闭到托盘」等主动隐藏不再被干扰——
/// 因此不会把用户刚藏起的窗口弹回来。
///
/// 该守卫是纯增量保险：正常启动（窗口秒现）首次轮询即 `is_visible == true`
/// 直接退出，零开销；上游修复后此函数可整体移除。
fn spawn_startup_window_guard(app: tauri::AppHandle) {
    const GUARD_MAX_MS: u64 = 20_000;
    const GUARD_INTERVAL_MS: u64 = 2_500;
    let spawn_result = std::thread::Builder::new()
        .name("startup-window-guard".into())
        .spawn(move || {
            let mut elapsed_ms = 0u64;
            let mut seen_visible = false;
            while elapsed_ms < GUARD_MAX_MS {
                std::thread::sleep(Duration::from_millis(GUARD_INTERVAL_MS));
                elapsed_ms += GUARD_INTERVAL_MS;
                let Some(window) = app.get_webview_window("main") else {
                    // 主窗口尚未创建（启动早期）或已销毁：跳过本轮
                    continue;
                };
                match window.is_visible() {
                    Ok(true) => {
                        seen_visible = true;
                    }
                    Ok(false) if !seen_visible => {
                        // 从未可见 = 疑似 show 被吞，重试唤回
                        log::warn!(
                            "启动守卫：主窗口启动 {elapsed_ms}ms 仍不可见，重试显示（macOS 26 上游缺陷缓解）"
                        );
                        reveal_main_window(&app);
                    }
                    Ok(false) => {
                        // 曾可见后被主动隐藏（关闭到托盘等）：尊重用户操作，不干预
                        return;
                    }
                    Err(e) => {
                        log::warn!("启动守卫：is_visible 查询失败: {e}");
                    }
                }
            }
            // 20s 仍从未可见：交回给用户（Dock/托盘仍可唤回），仅告警一次
            log::warn!("启动守卫：主窗口 20s 内未能确认可见，请通过 Dock/托盘图标唤回");
        });
    if let Err(e) = spawn_result {
        log::error!("startup-window-guard 线程创建失败: {e}");
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

    // 内置默认规则种子：分类种子之后执行（默认规则外键指向 builtin-document）。
    // 仅在 rules 表为空时补齐两条默认禁用规则（PDF/文本归档），失败不阻断启动。
    let seed_rule_result = {
        let db_guard = lock_db();
        RuleRepo::seed_default_rules(db_guard.conn())
    };
    match seed_rule_result {
        Ok(0) => log::info!("默认规则已存在或已有自定义规则，跳过种子"),
        Ok(n) => log::info!("默认规则种子：新增 {n} 条规则"),
        Err(e) => log::warn!("默认规则种子失败（不影响启动）: {e}"),
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

    // ---- Sidecar 二进制解析（仅定位，启动交给 setup 后台引导，P1-1）----
    //
    // 解析优先级：
    //   1) FILEMIND_SIDECAR_BINARY env 强制指定（CI / 调试覆盖）
    //   2) dev 布局解析（CARGO_MANIFEST_DIR / cwd 下的 repo `filemind/binaries/`）
    //   3) macOS/Windows 打包态：dev 找不到 → **不退出**，留空由后台引导按 Tauri
    //      `externalBin` 落地路径（主可执行文件同目录）启动，保证安装包在任意
    //      cwd（用户双击 / Spotlight 启动）下都能跑。
    // 其他平台无 bundle 兜底，dev 找不到直接退出（与旧行为一致）。
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
    // 引导线程用（setup 闭包 move 需要独立副本；env_override 直接 move 进闭包）
    let bootstrap_binary = sidecar_binary.clone();
    let mut sidecar_manager = SidecarManager::new(sidecar_binary.clone());

    // T7.4 云端代理（07-§4）：生成调用方共享 token → 在 setup 阶段启动本机代理
    // （127.0.0.1:8766）→ 按当前推理模式决定给 Sidecar 注入哪些云端 env（含脱敏开关）。
    //
    // ⚠️ 端口绑定（spawn_proxy_server）**必须延迟到 Tauri `Builder` 构建之后**（即
    // setup 回调内执行）：tauri-plugin-single-instance 的「唤醒已有实例并退出」逻辑
    // 只在 `builder.build()` 阶段才生效。若在 main 早期（插件生效前）抢先绑定 8766，
    // 当上一实例/残留进程已占用该端口时，第二实例会在插件检测前因 Address already
    // in use 直接闪退——用户表现为「点击启动无任何页面」。推迟后第二实例由单实例
    // 插件通知主实例 reveal 主窗口并干净退出，主实例 setup 绑定失败才中止。
    // token 生成失败仍即退出（安全边界初始化不可跳过，模式与 log_redact 一致）。
    let proxy_token = match generate_token() {
        Ok(token) => token,
        Err(e) => {
            log::error!("云端代理 token 生成失败: {e}");
            std::process::exit(1);
        }
    };
    let proxy_state =
        cloud_proxy::CloudProxyState::new(proxy_token.clone()).with_db(Arc::clone(&database));
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

    // P1-1：Sidecar 不再在本阶段启动——`setup` 内 `spawn_sidecar_bootstrap`
    // 起后台线程完成启动 + 握手（打包态冷启动 30s+ 不阻塞窗口显示）。
    // 失败收敛为 `sidecar-status` 事件 + 前端重试，不再 `process::exit`。
    log::info!("Sidecar 启动已异步化：由 setup 后台引导线程接管");

    // Tauri AppState 生命周期贯穿整个 Tauri 运行期，
    // 同时 main 栈变量也持有 AppState 引用直到 run() 返回；
    // 为了让 run() 返回后还能访问 manager/PSK/seq 做优雅关闭，这里 clone AppHandle：
    // 通过 AppHandle 就能从 managed state 再取出。
    let builder = tauri::Builder::default()
        // T6.1 单实例：第二个进程启动时回调里把已有窗口显示出来并聚焦，新进程随后退出
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            reveal_main_window(app);
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
            // P1-1：PSK 由后台引导线程握手成功后写入，初始为 None
            sidecar_psk: Mutex::new(None),
            sidecar_binary: Mutex::new(sidecar_binary),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
            // P1-1：初始 Starting，后台引导线程随后更新
            sidecar_status: Mutex::new(SidecarStatus::Starting),
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
            commands::file_ops::list_scanned_directories,
            commands::file_ops::remove_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::delete_files,
            commands::file_ops::undo_batch,
            commands::file_preview::read_file_preview,
            commands::file_preview::read_document_preview,
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
            // P1-1 Sidecar 生命周期查询 / 手动重试
            commands::sidecar::get_sidecar_status,
            commands::sidecar::retry_sidecar_start,
            // T9.5 E2E 测试专用命令（仅 debug 注册；release 不携带自动化入口）
            #[cfg(debug_assertions)]
            commands::e2e::e2e_get_test_dir,
        ])
        .setup(move |app| {
            // T7.4：云端代理在 127.0.0.1:8766 的绑定放到 setup 首步（见 main 处注释）：
            // 二次启动的第二实例在此阶段之前已被 single-instance 插件接管退出，不会
            // 因端口占用在无 UI 阶段闪退；此处只有主实例会执行，绑定失败才中止。
            if let Err(e) = cloud_proxy::spawn_proxy_server(proxy_state) {
                log::error!("云端代理启动失败（setup 阶段）: {e}");
                return Err(e.into());
            }
            log::info!("云端代理已就绪（setup）: {CLOUD_PROXY_HOST}:{CLOUD_PROXY_PORT}");

            // P1-1：Sidecar 后台引导（不阻塞窗口显示）。
            // 路径解析（dev/bundle/env）与启动+握手全部在后台线程完成，
            // 状态经 `sidecar-status` 事件推送；失败由前端展示 + 重试，不退出。
            spawn_sidecar_bootstrap(app.handle().clone(), bootstrap_binary, env_override);
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
                    reveal_main_window(tray.app_handle());
                })
                .build(app)?;

            // T6.1-bug：macOS 26 冷启动窗口偶发不可见的上游缺陷缓解守卫
            // （依赖 setup 已跑完、主窗口已由 config 创建；线程内按 20s 窗口轮询）
            spawn_startup_window_guard(app.handle().clone());
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

    // app 级事件（macOS Dock 激活 Reopen 等）需要自定义 run 回调分发；
    // 先 build 拿 App 再 `app.run`。build 失败 = Tauri 初始化级错误（资源/配置
    // 缺失），无 UI 可补救，打日志退出。
    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(e) => {
            log::error!("Error while building tauri application: {e}");
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| {
        // macOS Dock/Finder 图标激活已运行实例：若窗口被「关闭到托盘」（hide）后
        // 无可见窗口，标准 macOS 行为是唤回主窗口（tauri RunEvent::Reopen 仅 macOS 发射）。
        //
        // 修复「点红钮关闭后，从 Dock 再点打不开窗口」：不信任 macOS 的
        // has_visible_windows 标志——它对「已隐藏/迷你化」的窗口可能仍上报 true
        // （Apple 文档：迷你化窗口在 hasVisibleWindows 中算可见），旧实现
        // `has_visible_windows: false` 匹配不到导致唤回被跳过。改用主窗口实际
        // 可见性判断：只要当前不可见就唤回，窗口正可见时则不抢焦点。
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            let needs_reveal = app_handle
                .get_webview_window("main")
                .is_some_and(|w| !w.is_visible().unwrap_or(false));
            if needs_reveal {
                log::info!("Dock 图标激活：主窗口不可见，唤回主窗口");
                reveal_main_window(app_handle);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app_handle, event);
    });

    // 后备：若主窗口关闭事件路径未触发（极少，仅 headless/菜单退出等非 CloseRequested），
    // SidecarManager 此时由 Tauri managed state 析构 → Drop → stop_hard() 兜底杀一次。
    // 由于 stopped 标志位在 CloseRequested 主路径已经 set，Drop 不会重复 kill。
}
