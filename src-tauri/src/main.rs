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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use filemind_lib::commands;
use filemind_lib::db::{Database, OperationRepo};
use filemind_lib::error::AppError;
use filemind_lib::sidecar::{
    resolve_bundle_binary_path, resolve_dev_binary_path, SidecarManager, WatchdogAction,
};
use filemind_lib::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    Manager,
};

/// 健康看门狗基础轮询间隔（毫秒）。
///
/// 与 manager 内部 `HEALTH_POLL_INTERVAL_MS` 同值；集中到 main.rs 便于未来调优。
const WATCHDOG_TICK_MS: u64 = 1000;

/// 全局退出标志位：用于区分「X 关闭=最小化到托盘」与「托盘菜单退出=真退出」。
///
/// 默认 false → `CloseRequested` 走 hide 路径；托盘「退出」菜单先置 true 再触发关闭，
/// 此时 `CloseRequested` 走 `stop_graceful` 真退出路径。
static IS_QUITTING: AtomicBool = AtomicBool::new(false);

fn get_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".filemind").join("filemind.db")
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
                                    let state = app_handle.state::<AppState>();
                                    // 同步新 PSK 到 AppState 供 proxy.rs 后续签名使用
                                    if let Ok(mut psk_guard) = state.sidecar_psk.lock() {
                                        *psk_guard = Some(new_psk);
                                    }
                                    // 新 Sidecar 端 seq 从 0 开始，Rust 端必须跟随重置
                                    state.request_seq.store(0, Ordering::SeqCst);
                                    // 累计重启计数（排障用）
                                    let prev =
                                        state.sidecar_restart_count.fetch_add(1, Ordering::SeqCst);
                                    log::info!("Sidecar 重启成功，累计重启次数 = {}", prev + 1);
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
    env_logger::init();
    let db_path = get_db_path();

    let database = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            log::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    // T3.5：启动时校验操作日志链式哈希完整性，检测到篡改仅告警、不阻断启动
    match OperationRepo::verify_chain(database.conn()) {
        Ok(None) => log::info!("操作日志链式哈希校验通过"),
        Ok(Some(break_id)) => {
            log::error!("操作日志链式哈希校验失败，检测到篡改，断裂于记录 {break_id}");
        }
        Err(e) => log::error!("操作日志链式哈希校验出错: {e}"),
    }

    // 启动 Sidecar 并完成 HMAC 握手：失败直接退出，避免在未验证身份时进入主循环
    //
    // 解析 Sidecar 二进制路径（P1 阶段：只做 dev 路径解析；P2 阶段增加 AppHandle
    // bundle 路径覆盖 + AppState.sidecar_binary 字段暴露）。
    // FILEMIND_SIDECAR_BINARY env 存在则优先生效，便于 CI / 调试覆盖。
    let sidecar_binary =
        match resolve_dev_binary_path(std::env::var("FILEMIND_SIDECAR_BINARY").ok().as_deref()) {
            Ok(p) => p,
            Err(e) => {
                log::error!("Sidecar 二进制解析失败: {e}");
                std::process::exit(1);
            }
        };
    log::info!("Sidecar binary path: {}", sidecar_binary.display());
    let mut sidecar_manager = SidecarManager::new(sidecar_binary.clone());
    let sidecar_psk = match start_sidecar_with_handshake(&mut sidecar_manager) {
        Ok(psk) => Some(psk),
        Err(e) => {
            log::error!("Sidecar handshake failed: {e}");
            std::process::exit(1);
        }
    };
    // 主流程 manager 当前已持有 PSK（start_with_handshake 内部已存进 self.psk），
    // 若与 AppState 写入的 psk 不一致以 AppState 为准，这里同步拷贝一次保持一致。
    debug_assert!(sidecar_manager
        .psk()
        .is_none_or(|inner| Some(inner) == sidecar_psk.as_deref()));

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
        .manage(AppState {
            db: Mutex::new(database),
            sidecar_manager: Mutex::new(sidecar_manager),
            sidecar_psk: Mutex::new(sidecar_psk),
            sidecar_binary: Mutex::new(sidecar_binary),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
        })
        .invoke_handler(tauri::generate_handler![
            commands::file_ops::scan_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::undo_batch,
            commands::file_query::list_files,
            commands::file_query::search_files,
            commands::file_query::search_by_filename,
            commands::file_query::get_file_stats,
            commands::file_query::update_file_category,
            commands::file_query::get_operation_history,
            commands::file_query::get_batch_detail,
            commands::inference::get_inference_mode,
            commands::inference::set_inference_mode,
            commands::config::get_config,
            commands::config::update_config,
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
                                log::warn!("bundle manager 启动+握手失败，回退沿用 dev 路径（已可用）: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("未命中 bundle Sidecar 路径（可能是 dev 环境），保持 dev 路径: {e}");
                    }
                }
            }
            spawn_watchdog(app.handle().clone());

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
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        // 置真退出标志 → 触发主窗口关闭 → CloseRequested 走 stop_graceful 路径
                        IS_QUITTING.store(true, Ordering::SeqCst);
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.close();
                        } else {
                            // 无主窗口（极少）：直接退出 app
                            app.exit(0);
                        }
                    }
                    _ => {}
                })
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
            if IS_QUITTING.load(Ordering::SeqCst) {
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
