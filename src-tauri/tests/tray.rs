//! T6.1 托盘菜单集成测试（E2E-006 托盘替代）。
//!
//! 背景：`WDIO`/`WebDriver` 只驱动 `WebView`，**无法点击原生托盘菜单**（macOS 菜单栏图标、
//! Windows/Linux 托盘图标均为系统级 UI）。因此托盘交互改在 Rust 层覆盖：
//! - `tray_menu_action` 纯决策映射（不依赖运行时，可单测）
//! - `handle_tray_menu_event` 副作用：用 `tauri::test::mock_app`（`MockRuntime`，
//!   `test` feature）注入「退出」菜单，断言 `IS_QUITTING` 被置位（真退出信号）。
//!
//! 注意：`IS_QUITTING` 是全局 static，多个测试并行跑会互相干扰，故副作用断言
//! 全部收进单一顺序测试函数内。

use std::sync::atomic::Ordering;

use filemind_lib::tray::{handle_tray_menu_event, tray_menu_action, TrayAction};
use tauri::test::mock_app;
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[test]
fn menu_id_to_action_mapping() {
    // 纯决策：菜单 ID → 动作（与 src/tray.rs 内部单测等价，集成层再固化一次契约）
    assert_eq!(tray_menu_action("show"), TrayAction::Show);
    assert_eq!(tray_menu_action("quit"), TrayAction::Quit);
    assert_eq!(tray_menu_action("unknown"), TrayAction::Ignore);
    assert_eq!(tray_menu_action(""), TrayAction::Ignore);
}

#[test]
fn menu_events_set_and_clear_quitting_flag() -> Result<(), Box<dyn std::error::Error>> {
    // MockRuntime 支持 create_window（窗口 dispatcher 的 close/show 均已实现），
    // 但 `request_exit` 是 `unimplemented!()`——因此必须提供 "main" 窗口，让 Quit
    // 走 `window.close()` 分支而非 `app.exit(0)` 兜底。
    let app = mock_app();
    WebviewWindowBuilder::new(app.handle(), "main", WebviewUrl::default()).build()?;

    // 初始态：false
    filemind_lib::tray::IS_QUITTING.store(false, Ordering::SeqCst);

    // 「退出」→ 真退出标志置位
    handle_tray_menu_event(app.handle(), "quit");
    assert!(
        filemind_lib::tray::IS_QUITTING.load(Ordering::SeqCst),
        "quit 菜单应置 IS_QUITTING=true"
    );

    // 复位，验证 show/ignore 不触碰该标志
    filemind_lib::tray::IS_QUITTING.store(false, Ordering::SeqCst);
    handle_tray_menu_event(app.handle(), "show");
    handle_tray_menu_event(app.handle(), "unknown");
    assert!(
        !filemind_lib::tray::IS_QUITTING.load(Ordering::SeqCst),
        "show/ignore 不应改动 IS_QUITTING"
    );
    Ok(())
}
