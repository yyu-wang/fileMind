//! 系统托盘菜单处理（T6.1）：菜单 ID → 动作的纯决策 + 副作用执行。
//!
//! 与 `main.rs` 分离的原因：
//! - **可测性**：WDIO 无法点击原生托盘菜单（WebDriver 只驱动 `WebView`），
//!   托盘交互改在 Rust 层以集成测试覆盖（`tests/tray.rs`）；
//! - **单一职责**：`main.rs` 的 `setup` 只做托盘构建，动作决策集中在 `tray_menu_action`。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, Runtime};

/// 真退出标志：托盘「退出」菜单置位后，主窗口 `CloseRequested` 走优雅关停而非最小化。
pub static IS_QUITTING: AtomicBool = AtomicBool::new(false);

/// 托盘菜单项对应的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// 显示并聚焦主窗口。
    Show,
    /// 置真退出标志并关闭主窗口。
    Quit,
    /// 未识别菜单项，忽略。
    Ignore,
}

/// 纯决策：菜单 ID → 动作。不依赖 Tauri 运行时，可单独单测。
#[must_use]
pub fn tray_menu_action(menu_id: &str) -> TrayAction {
    match menu_id {
        "show" => TrayAction::Show,
        "quit" => TrayAction::Quit,
        _ => TrayAction::Ignore,
    }
}

/// 执行托盘菜单动作（副作用集中于此；菜单事件回调只做分发）。
///
/// 泛型 `R: Runtime` 允许用 `tauri::test::mock_app`（MockRuntime）注入测试，
/// 而生产路径由 Tauri 自动推断为 `Wry`。
pub fn handle_tray_menu_event<R: Runtime>(app: &AppHandle<R>, menu_id: &str) {
    match tray_menu_action(menu_id) {
        TrayAction::Show => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        TrayAction::Quit => {
            IS_QUITTING.store(true, Ordering::SeqCst);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.close();
            } else {
                // 无主窗口（极少）：直接退出 app
                app.exit(0);
            }
        }
        TrayAction::Ignore => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{tray_menu_action, TrayAction};

    #[test]
    fn maps_known_menu_ids() {
        assert_eq!(tray_menu_action("show"), TrayAction::Show);
        assert_eq!(tray_menu_action("quit"), TrayAction::Quit);
    }

    #[test]
    fn ignores_unknown_menu_ids() {
        assert_eq!(tray_menu_action("unknown"), TrayAction::Ignore);
        assert_eq!(tray_menu_action(""), TrayAction::Ignore);
    }
}
