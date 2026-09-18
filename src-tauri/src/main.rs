//! `FileMind` 桌面应用入口：初始化数据库、启动 Sidecar 与握手、注册 IPC 命令并启动 Tauri。
//!
//! 生命周期（含 Sidecar，T1.5 + T6.1 窗口/托盘管理）：
//! 1. 启动阶段：建 `SidecarManager` → `start_with_handshake` 成功 → 把 manager / PSK
//!    / seq / 重启计数放进 `AppState`
//! 2. 运行期：`setup` 中构建系统托盘 + spawn 后台 `watchdog`（独立 current-thread
//!    `tokio` runtime），每秒 tick：连续健康失败或 `Child::try_wait` 已退出 → 指数退避后
//!    `restart()`，并同步更新 `AppState` 里的 PSK + `reset` seq；1 分钟内 10 次重启 →
//!    `CrashLoop` 暂停，打 `error` 日志后 watchdog 自动退为「仅告警，不再自动恢复」
//! 3. 退出：Rust 侧三条入口统一收口到 `stop_sidecar_blocking`（`lib.rs`）
//!    - 窗口 `CloseRequested` 且 `IS_QUITTING=false` → 阻止关闭 + `hide()` 最小化到托盘
//!    - 窗口 `CloseRequested` 且 `IS_QUITTING=true`（托盘「退出」菜单置位）→ 优雅关停后退出
//!    - `RunEvent::ExitRequested`（`AppHandle::exit()` / 最后一个窗口被销毁）→
//!      先置 `IS_QUITTING` 再关停（置位是为了让随后的 `CloseRequested` 不再走托盘分支）
//!      ⚠️ 注意 **macOS ⌘Q 不在此列**：`AppKit` terminate 直接 `exit()`，Rust 侧收不到
//!      任何回调；该路径由 Sidecar 自身的父进程死亡看门狗退出
//!      （`python-sidecar/app/core/parent_watchdog.py`）
//! 4. `Drop` 兜底：若正常退出路径全被跳过（仅 run 内部 panic），`Drop` 用 `stopped` 标志位
//!    保证仅一次 hard kill，不重复杀进程
//!
//! 单实例：`tauri-plugin-single-instance` 防止二次启动产生孤儿 sidecar，第二次启动
//! 时回调里把已有主窗口显示出来并聚焦，新进程随后退出。
//!
//! 模块划分（原单文件 703 行按职责拆分，各文件 < 300 行，见 `rules/complexity.md`）：
//!   - `startup`       日志脱敏 / DB 打开 / 内置种子 / 链校验 / E2E 预置
//!   - `sidecar_setup` Sidecar 二进制解析 / 云端代理 env 装配 / 孤儿清理
//!   - `watchdog`      健康看门狗（退避重启 / `CrashLoop` 降级）
//!   - `window`        启动可见性守卫 / 托盘 / 窗口关闭与退出事件
//!   - `ipc_handler`   `generate_handler!` 命令清单

mod ipc_handler;
mod sidecar_setup;
mod startup;
mod watchdog;
mod window;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use filemind_lib::security::cloud_proxy::{self, CLOUD_PROXY_HOST, CLOUD_PROXY_PORT};
use filemind_lib::sidecar::{spawn_sidecar_bootstrap, SidecarManager};
use filemind_lib::tray::reveal_main_window;
use filemind_lib::{AppState, SidecarStatus};

/// 应用入口：按「启动初始化 → Sidecar 装配 → Tauri 构建 → 事件循环」串联各模块。
///
/// `generate_context!` 宏在编译期生成较大的上下文结构体（框架行为），栈占用为 Tauri
/// 已知模式，非业务代码问题，定点豁免此 nursery lint。
#[allow(clippy::large_stack_frames)]
fn main() {
    // ---- 启动期一次性初始化（日志 → DB → 种子 → 链校验 → E2E 预置）----
    startup::init_logging();
    let database = startup::open_database();
    startup::seed_categories(&database);
    startup::seed_default_rules(&database);
    #[cfg(debug_assertions)]
    startup::apply_e2e_config(&database);
    startup::verify_operation_chain(&database);

    // ---- Sidecar 装配（只定位与配置，启动交给 setup 后台引导，P1-1）----
    let (sidecar_binary, env_override) = sidecar_setup::resolve_binary();
    // 引导线程用（setup 闭包 move 需要独立副本；env_override 直接 move 进闭包）
    let bootstrap_binary = sidecar_binary.clone();
    let mut sidecar_manager = SidecarManager::new(sidecar_binary.clone());

    // T7.4 云端代理（07-§4）：装配 token 与 env 并返回代理状态；
    // 端口绑定必须延迟到 setup（原因见 `sidecar_setup::configure_cloud_env` 注释）。
    let proxy_state = sidecar_setup::configure_cloud_env(&mut sidecar_manager, &database);

    // T3b：本地生成后端配置（backend / GGUF 标识）——Sidecar 不读 SQLite，由此注入
    sidecar_setup::configure_local_llm_env(&mut sidecar_manager, &database);

    // BE-M3：启动前清理上次异常退出残留的孤儿 Sidecar，防止旧进程占住 8765 端口
    sidecar_setup::cleanup_orphans();

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
        .invoke_handler(ipc_handler::build())
        .setup(move |app| {
            // T7.4：云端代理在 127.0.0.1:8766 的绑定放到 setup 首步（原因见
            // `sidecar_setup::configure_cloud_env` 注释）：二次启动的第二实例在此阶段之前
            // 已被 single-instance 插件接管退出，不会因端口占用在无 UI 阶段闪退；
            // 此处只有主实例会执行，绑定失败才中止。
            if let Err(e) = cloud_proxy::spawn_proxy_server(proxy_state) {
                log::error!("云端代理启动失败（setup 阶段）: {e}");
                return Err(e.into());
            }
            log::info!("云端代理已就绪（setup）: {CLOUD_PROXY_HOST}:{CLOUD_PROXY_PORT}");

            // P1-1：Sidecar 后台引导（不阻塞窗口显示）。
            // 路径解析（dev/bundle/env）与启动+握手全部在后台线程完成，
            // 状态经 `sidecar-status` 事件推送；失败由前端展示 + 重试，不退出。
            spawn_sidecar_bootstrap(app.handle().clone(), bootstrap_binary, env_override);
            watchdog::spawn(app.handle().clone());

            // T9.5 E2E：`FILEMIND_E2E=1` 时强制显示主窗口（仅 debug 构建编译）
            #[cfg(debug_assertions)]
            window::force_show_main_window(app);

            // T6.1 系统托盘：菜单「显示主窗口 / 退出」+ 左键点击显示窗口
            window::build_tray(app)?;

            // T6.1-bug：macOS 26 冷启动窗口偶发不可见的上游缺陷缓解守卫
            // （依赖 setup 已跑完、主窗口已由 config 创建；线程内按 20s 窗口轮询）
            window::spawn_startup_guard(app.handle().clone());
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
    let builder = builder.on_window_event(window::handle_window_event);

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

    app.run(window::handle_run_event);

    // 兜底：若上述退出入口全未触发（仅 run 内部 panic 等异常路径），SidecarManager
    // 此时随 Tauri managed state 析构 → Drop → stop_hard() 硬杀一次。
    // stopped 标志位已在任一正常路径置位，Drop 不会重复 kill。
}
