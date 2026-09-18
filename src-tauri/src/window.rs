//! 窗口与托盘生命周期：启动可见性自愈守卫、托盘菜单、窗口关闭语义、应用级退出/唤回事件。
//!
//! 退出语义（三条入口统一收口到 `stop_sidecar_blocking`）：
//! - 窗口 `CloseRequested` 且 `IS_QUITTING=false` → 阻止关闭 + `hide()` 最小化到托盘
//! - 窗口 `CloseRequested` 且 `IS_QUITTING=true`（托盘「退出」菜单置位）→ 优雅关停后退出
//! - `RunEvent::ExitRequested`（`AppHandle::exit()` / 最后一个窗口被销毁）→ 先置
//!   `IS_QUITTING` 再关停（置位是为了让随后的 `CloseRequested` 不再走托盘分支）
//!
//! ⚠️ macOS ⌘Q 不在此列：AppKit terminate 直接 `exit()`，Rust 侧收不到任何回调；
//! 该路径由 Sidecar 自身的父进程死亡看门狗退出（`python-sidecar/app/core/parent_watchdog.py`）。

use std::sync::atomic::Ordering;
use std::time::Duration;

use filemind_lib::stop_sidecar_blocking;
use filemind_lib::tray::{handle_tray_menu_event, mark_quitting, reveal_main_window, IS_QUITTING};
use tauri::menu::{Menu, MenuItem};
use tauri::{Manager, RunEvent, WindowEvent};

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
pub fn spawn_startup_guard(app: tauri::AppHandle) {
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

/// T6.1 系统托盘：菜单「显示主窗口 / 退出」+ 托盘图标点击唤回主窗口。
pub fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}

/// 窗口关闭事件：按 `IS_QUITTING` 区分「最小化到托盘」与「真退出」。
pub fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if IS_QUITTING.load(Ordering::SeqCst) {
            stop_sidecar_blocking(window.app_handle());
        } else {
            // 最小化到托盘：阻止默认关闭，仅隐藏窗口
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

/// 应用级事件分发：`ExitRequested` 优雅关停；macOS `Reopen`（Dock/Finder 激活）唤回主窗口。
///
/// ⚠️ 不要指望这里能兜住 macOS ⌘Q——AppKit 的 `-[NSApplication terminate:]` 直接
/// `exit()`，且 tao 的 macOS 后端不实现 `applicationShouldTerminate`，事件循环根本收不到
/// 通知（真机实测确认）；该路径由 Sidecar 自身的父进程死亡看门狗负责。
pub fn handle_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
    // 应用级退出请求：`AppHandle::exit()` / 最后一个窗口被销毁时触发（tauri 仅在
    // 这两种情形发出本事件）。
    //
    // 先置 IS_QUITTING：`ExitRequested` 之后 Tauri 仍会逐个关闭窗口，标志位为 false 时
    // `CloseRequested` 会 `prevent_close()` + `hide()` 把退出挡下来。关停本身幂等
    // （`stop_graceful` 的 `stopped` 标志位），两条路径交叉无副作用。
    if let RunEvent::ExitRequested { .. } = event {
        log::info!("收到应用退出请求：置真退出标志并优雅关停 Sidecar");
        mark_quitting();
        stop_sidecar_blocking(app_handle);
    }

    // macOS Dock/Finder 图标激活已运行实例：若窗口被「关闭到托盘」（hide）后无可见窗口，
    // 标准 macOS 行为是唤回主窗口（tauri `RunEvent::Reopen` 仅 macOS 发射）。
    //
    // 修复「点红钮关闭后，从 Dock 再点打不开窗口」：不信任 macOS 的
    // has_visible_windows 标志——它对「已隐藏/迷你化」的窗口可能仍上报 true
    // （Apple 文档：迷你化窗口在 hasVisibleWindows 中算可见），旧实现
    // `has_visible_windows: false` 匹配不到导致唤回被跳过。改用主窗口实际可见性判断：
    // 只要当前不可见就唤回，窗口正可见时则不抢焦点。
    #[cfg(target_os = "macos")]
    if let RunEvent::Reopen { .. } = event {
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
}

/// T9.5 E2E：`FILEMIND_E2E=1` 时强制显示主窗口。
///
/// 仅 debug 构建编译（release 不携带自动化入口）：避免依赖前端 `main.tsx` 的 `show()`
/// 是否成功，让 E2E 只关注被测行为。
#[cfg(debug_assertions)]
pub fn force_show_main_window(app: &tauri::App) {
    if !std::env::var("FILEMIND_E2E").is_ok_and(|v| v == "1") {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.show() {
            log::warn!("T9.5 E2E：强制显示主窗口失败: {e}");
        } else {
            log::info!("T9.5 E2E：FILEMIND_E2E=1 已强制显示主窗口");
        }
    }
}
