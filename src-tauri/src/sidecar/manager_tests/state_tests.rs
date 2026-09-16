//! 状态机相关测试：未启动/已停止语义、退避曲线、崩溃循环窗口、看门狗空转。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达（生产代码严格禁止）。

use std::time::Duration;

use super::super::*;
use super::support::*;

#[test]
fn test_new_has_no_process() {
    let mut manager = SidecarManager::new(stub_binary());
    let result = manager.stop_hard();
    assert!(result.is_ok(), "未启动时 stop_hard 应无副作用");
    assert_eq!(manager.psk(), None);
    assert_eq!(manager.pid(), None);
    assert!(!manager.is_stopped());
    assert_eq!(manager.binary_path_inner(), stub_binary().as_path());
}

#[test]
fn test_stopped_flag_idempotent_stop_graceful_path() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    rt.block_on(async {
        let r1 = manager.stop_graceful(1).await;
        let r2 = manager.stop_graceful(2).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok(), "第二次 stop_graceful 应直接 return Ok（幂等）");
    });
    assert!(manager.is_stopped());
}

#[test]
fn test_backoff_grows_exponentially_with_cap() {
    let mut manager = SidecarManager::new(stub_binary());
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_BASE_MS)
    );
    for i in 0u32..=10u32 {
        manager.consecutive_failures = i;
        let shift = i.min(3);
        let expected_ms = u64::min(
            RESTART_BACKOFF_BASE_MS * (1u64 << shift),
            RESTART_BACKOFF_CAP_MS,
        );
        assert_eq!(
            manager.next_backoff(),
            Duration::from_millis(expected_ms),
            "第 {i} 次连续失败 backoff 应为 {expected_ms}ms"
        );
    }
    manager.consecutive_failures = 100;
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_CAP_MS)
    );
}

#[test]
fn test_crash_loop_window_pauses_after_threshold() {
    let mut manager = SidecarManager::new(stub_binary());
    let now = Instant::now();
    for _ in 0..(CRASH_LOOP_MAX_RESTARTS - 1) {
        manager.recent_restarts.push_back(now);
    }
    assert!(
        !manager.is_crash_loop_paused(),
        "未达阈值前不应暂停：count={}/{}",
        manager.restart_count_in_window(),
        CRASH_LOOP_MAX_RESTARTS
    );
    manager.recent_restarts.push_back(now);
    assert!(
        manager.is_crash_loop_paused(),
        "到阈值后应进入 crash loop 暂停"
    );
}

#[test]
fn test_crash_loop_window_expires_old_entries() {
    let mut manager = SidecarManager::new(stub_binary());
    let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
    let old_t = Instant::now()
        .checked_sub(window + Duration::from_secs(1))
        .expect("系统时钟不支持 checked_sub");
    for _ in 0..CRASH_LOOP_MAX_RESTARTS {
        manager.recent_restarts.push_back(old_t);
    }
    let new_t = Instant::now();
    manager.recent_restarts.push_back(new_t);
    manager.recent_restarts.push_back(new_t);
    assert_eq!(
        manager.restart_count_in_window(),
        2,
        "超出窗口的旧条目应被移除，剩下应为新的 2 条"
    );
    assert!(
        !manager.is_crash_loop_paused(),
        "旧条目过期后不应再触发暂停"
    );
}

#[test]
fn test_watchdog_stopped_flag_returns_idle() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    let _ = manager.stopped.swap(true, Ordering::SeqCst);
    rt.block_on(async {
        let action = manager.watchdog_tick().await.expect("stopped 时应 ok");
        assert_eq!(action, WatchdogAction::Idle);
    });
}

#[test]
fn test_watchdog_unstarted_returns_idle_without_probing() {
    // P1-1：process=None 且 psk=None（后台引导中 / 启动失败等待重试）时，
    // watchdog 必须直接 Idle，不能触发 health 探测 → NeedRestart 双开进程。
    // 本机无 Sidecar 监听 8765，若守卫缺失会走 health 失败计数，此处验证不依赖网络。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    rt.block_on(async {
        let action = manager.watchdog_tick().await.expect("未启动时应 ok");
        assert_eq!(action, WatchdogAction::Idle);
    });
    assert_eq!(manager.recent_health_fails, 0, "未启动不应累计 health 失败");
}

#[test]
fn test_default_equals_new() {
    // Default/New 目前不派生 PartialEq（Child 不实现），退化为逐字段等价校验：
    // 都能 stop_hard 无副作用、都是 stopped=false、recent_restarts 空。
    let mut a = SidecarManager::new(stub_binary());
    let mut b = SidecarManager::default();
    assert!(a.stop_hard().is_ok());
    assert!(b.stop_hard().is_ok());
    assert_eq!(a.recent_restarts.len(), 0);
    assert_eq!(b.recent_restarts.len(), 0);
    assert!(!a.is_stopped());
    assert!(!b.is_stopped());
    assert!(
        b.binary_path_inner().is_file(),
        "default 占位 binary 必须是存在的文件"
    );
}
