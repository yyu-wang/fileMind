//! Sidecar 健康看门狗：独立线程内的 tick 循环，负责健康检查、指数退避重启与 `CrashLoop` 降级。
//!
//! 每次 tick 取 `AppHandle::state::<AppState>()`（`State<T>` 由 Tauri 持有，`Arc<AppState>`
//! 不现实），并让锁定 `sidecar_manager` 的时间尽可能短：`NeedRestart` 动作先释放 Mutex →
//! sleep backoff → 重新加锁 restart，避免持锁期间 `tokio::sleep` 阻塞其他线程访问
//! `AppState`。

use std::sync::atomic::Ordering;
use std::time::Duration;

use filemind_lib::error::AppError;
use filemind_lib::sidecar::{update_sidecar_status, WatchdogAction};
use filemind_lib::{AppState, SidecarStatus};
use tauri::{Emitter, Manager};

/// 健康看门狗基础轮询间隔（毫秒）。
///
/// 与 manager 内部 `HEALTH_POLL_INTERVAL_MS` 同值；集中到这里便于未来调优。
const WATCHDOG_TICK_MS: u64 = 1000;

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

/// 循环步进结果：`Stop` 表示 watchdog 应整体退出（Mutex 中毒等不可恢复状态）。
enum LoopFlow {
    /// 继续下一轮 tick。
    Continue,
    /// 结束循环，释放线程。
    Stop,
}

/// 构建 watchdog 用的 current-thread runtime（失败打日志并返回 `None`）。
fn build_runtime() -> Option<tokio::runtime::Runtime> {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => Some(rt),
        Err(e) => {
            log::error!("watchdog tokio runtime 初始化失败: {e}");
            None
        }
    }
}

/// `NeedRestart` 分支：取退避 → sleep → 重启并同步 PSK / 状态。
///
/// 允许 `await_holding_lock`：理由同 `spawn`——current-thread runtime 下 await 不跨线程
/// 调度，且 manager 持锁时间仅 `/health` 请求级别（几十毫秒），不阻塞 Tauri UI。
///
/// 允许 `future_not_send`：本 future 持有 `MutexGuard` 跨 await，因而不是 `Send`。它只被
/// `spawn` 内 `rt.block_on` 在**专用线程**上轮询（current-thread runtime，从不交给多线程
/// 调度器/`tokio::spawn`），拿不到 `Send` 也不影响任何调用点；若要消除该豁免，只能把
/// 逻辑摊回 `spawn` 的 async 块里（`async fn` 才受该 lint 约束），那会让 `spawn` 重新
/// 涨回 90 行以上。
///
/// 返回 `LoopFlow::Stop` 表示 `sidecar_manager` Mutex 中毒：调用方应结束循环，
/// 与拆分前「直接 return 出整个 async 块」的语义一致。
#[allow(clippy::await_holding_lock, clippy::future_not_send)]
async fn handle_need_restart(app_handle: &tauri::AppHandle) -> LoopFlow {
    // 先取 backoff（持锁时间短）
    let backoff = {
        let state = app_handle.state::<AppState>();
        let Ok(manager) = state.sidecar_manager.lock() else {
            return LoopFlow::Stop;
        };
        manager.next_backoff()
    };
    log::warn!("Sidecar 需要重启，退避等待 {backoff:?} 后开始");
    tokio::time::sleep(backoff).await;

    // 阶段 B：重启（持锁，期间阻塞其他方访问 manager 可接受）
    // 注：直接在外层 runtime 上 await——嵌套 `block_on`（旧实现：此处新建 inner_rt
    // 并 block_on）会触发 tokio panic「Cannot start a runtime from within a runtime」，
    // 且 release profile 为 panic=abort，整进程直接闪退（回归：打包版首次提问时
    // sidecar 加载 rerank 模型饥饿事件循环 → /health 连续失败 → watchdog 重启 → 崩溃）。
    let restart_result: Result<Vec<u8>, AppError> = {
        let state = app_handle.state::<AppState>();
        let Ok(mut manager) = state.sidecar_manager.lock() else {
            return LoopFlow::Stop;
        };
        manager.restart().await
    };
    match restart_result {
        Ok(new_psk) => {
            // 同步新 PSK / seq / 计数（失败告警见 `on_restart_success` 注释）
            on_restart_success(&app_handle.state::<AppState>(), new_psk);
            // P1-1：重启成功同步前端状态（事件 + AppState）
            update_sidecar_status(app_handle, SidecarStatus::Ready);
        }
        Err(e) => {
            log::error!("Sidecar 重启失败: {e}");
            // P1-1：单次重启失败即转 Failed 交给用户重试，不再无限退避重试
            // （重启后 process/psk 均为 None，watchdog 守卫会停在 Idle，见 manager）
            update_sidecar_status(app_handle, SidecarStatus::Failed(e.to_string()));
        }
    }
    LoopFlow::Continue
}

/// `CrashLoop` 分支：置 Failed、通知前端「自动恢复已暂停」，并按分钟级节奏告警。
async fn handle_crash_loop(
    app_handle: &tauri::AppHandle,
    count: u32,
    window_secs: u32,
    message: &str,
) {
    log::error!("Sidecar 进入 CrashLoop（{count}/{window_secs}s）：{message}");
    // P1-1：同步为 Failed，前端展示错误 + 重试入口
    update_sidecar_status(app_handle, SidecarStatus::Failed(message.to_string()));
    // BE-M7：通知前端展示「自动恢复已暂停」提示；本分支自带 60s sleep，
    // 事件至多每分钟一条不会刷屏
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

/// 在独立线程中启动健康看门狗循环（线程内自建 current-thread runtime）。
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
pub fn spawn(app_handle: tauri::AppHandle) {
    let spawn_result = std::thread::Builder::new()
        .name("sidecar-watchdog".into())
        .spawn(move || {
            let Some(rt) = build_runtime() else {
                return;
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
                            if matches!(handle_need_restart(&app_handle).await, LoopFlow::Stop) {
                                return;
                            }
                        }
                        Err(AppError::SidecarCrashLoop {
                            count,
                            window_secs,
                            message,
                        }) => {
                            handle_crash_loop(&app_handle, count, window_secs, &message).await;
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
