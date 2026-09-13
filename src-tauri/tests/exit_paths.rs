//! 应用退出路径集成测试：`stop_sidecar_blocking`（托盘退出 / Cmd+Q 共用的唯一收口）。
//!
//! 背景（macOS 真机复现的 T6.1 遗留缺陷）：`Cmd+Q` 走 AppKit terminate，只发
//! `RunEvent::ExitRequested` 而**不发**窗口 `CloseRequested`；修复前优雅关停只挂在
//! `CloseRequested` 分支上，导致 Cmd+Q 后 Sidecar 变孤儿（`ppid=1`）继续占住 8765。
//!
//! `main.rs` 的事件回调需要真实事件循环才能驱动，无法单测；因此把「取 state →
//! 关停 Sidecar」抽成 lib 函数，用 `tauri::test::mock_app`（`MockRuntime`）注入
//! managed `AppState` 后在此固化契约：关停标志位 + 防重放序号递增 + 可重入。
//!
//! 注意：`SidecarManager` 未启动进程时，`stop_graceful` 的第 2 阶段会等满
//! `GRACEFUL_SELF_EXIT_SECS`（3s）才落 `stop_hard`，故单个用例耗时约 3s。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use filemind_lib::db::Database;
use filemind_lib::sidecar::SidecarManager;
use filemind_lib::{stop_sidecar_blocking, AppState, SidecarStatus};
use tauri::test::mock_app;
use tauri::Manager;

/// 构造最小可用 `AppState`：DB 指向临时文件，Sidecar 用占位路径且不启动进程。
fn make_app_state(db_path: &std::path::Path) -> Result<AppState, Box<dyn std::error::Error>> {
    Ok(AppState {
        db: Arc::new(Mutex::new(Database::open(db_path)?)),
        sidecar_manager: Mutex::new(SidecarManager::new("/dev/null/sidecar-nonexistent".into())),
        sidecar_psk: Mutex::new(None),
        sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
        request_seq: AtomicU64::new(0),
        sidecar_restart_count: AtomicU64::new(0),
        sidecar_status: Mutex::new(SidecarStatus::Starting),
    })
}

/// 读 `sidecar_manager.stopped`（锁中毒时视为未关停，避免用例静默通过）。
fn is_manager_stopped(state: &AppState) -> bool {
    state
        .sidecar_manager
        .lock()
        .map(|manager| manager.is_stopped())
        .unwrap_or(false)
}

/// 退出请求收口：关停标志位置位 + `request_seq` 递增（防重放序号必须前进）。
#[test]
fn stops_manager_and_bumps_request_seq() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let app = mock_app();
    app.manage(make_app_state(tmp_db.path())?);
    let handle = app.handle().clone();

    stop_sidecar_blocking(&handle);

    let state = handle.state::<AppState>();
    assert!(
        is_manager_stopped(&state),
        "退出收口必须把 SidecarManager 标记为已关停"
    );
    assert_eq!(
        state.request_seq.load(Ordering::SeqCst),
        1,
        "退出收口应递增 request_seq 一次"
    );
    Ok(())
}

/// 可重入：`ExitRequested` 与随后的 `CloseRequested` 会先后触发同一收口，
/// 第二次必须是 `stop_graceful` 的幂等空操作（不 panic、不死锁、不重复 kill）。
#[test]
fn repeated_calls_are_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_db = tempfile::NamedTempFile::new()?;
    let app = mock_app();
    app.manage(make_app_state(tmp_db.path())?);
    let handle = app.handle().clone();

    stop_sidecar_blocking(&handle);
    stop_sidecar_blocking(&handle);

    let state = handle.state::<AppState>();
    assert!(
        is_manager_stopped(&state),
        "重复调用后 SidecarManager 仍应为已关停"
    );
    // 序号每次调用都前进（收口入口无条件 fetch_add），幂等性由 stopped 标志位保证
    assert_eq!(
        state.request_seq.load(Ordering::SeqCst),
        2,
        "两次退出收口应递增 request_seq 两次"
    );
    Ok(())
}
