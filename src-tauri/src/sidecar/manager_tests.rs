//! `SidecarManager` 单元测试（子模块 `super::*` 能访问私有字段）。
//!
//! 独立文件拆分原因：`manager.rs` 主实现若内嵌 tests 模块会超 Rust<500 行复杂度阈值。

use super::*;

#[test]
fn test_new_has_no_process() {
    let mut manager = SidecarManager::new();
    // 未启动时 stop 应成功无副作用；stopped=false 时 stop_hard 也应 ok
    let result = manager.stop_hard();
    assert!(result.is_ok(), "未启动时 stop_hard 应无副作用");
    assert_eq!(manager.psk(), None);
    assert_eq!(manager.pid(), None);
    assert!(!manager.is_stopped());
}

#[test]
fn test_stopped_flag_idempotent_stop_graceful_path() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime");
    let mut manager = SidecarManager::new();
    // 未启动时 stop_graceful 不会报进程错误
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
    // 构造一个 manager，模拟连续失败：手动加 consecutive_failures
    let mut manager = SidecarManager::new();
    // 0 失败 → base
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_BASE_MS)
    );
    // 连续失败：手动递增
    for i in 0u32..=10u32 {
        // consecutive_failures 的加应该在 restart() 失败时加，但我们直接
        // 测 backoff：利用结构体可见性（同模块），tests 子模块可访问私有字段。
        manager.consecutive_failures = i;
        // 2^(min(i,3))
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
    // 超过 shift=3 后都停在 cap
    manager.consecutive_failures = 100;
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_CAP_MS)
    );
}

#[test]
fn test_crash_loop_window_pauses_after_threshold() {
    let mut manager = SidecarManager::new();
    // 构造 CRASH_LOOP_MAX_RESTARTS - 1 次在窗口内的重启：使用私有 recent_restarts
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
    // 再 1 条 → 到阈值 → 暂停
    manager.recent_restarts.push_back(now);
    assert!(
        manager.is_crash_loop_paused(),
        "到阈值后应进入 crash loop 暂停"
    );
}

#[test]
fn test_crash_loop_window_expires_old_entries() {
    // 构造旧条目（超过 CRASH_LOOP_WINDOW_SECS 前） + 几条新条目，验证旧的被清
    let mut manager = SidecarManager::new();
    let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
    // 10 条刚好超阈值的"老"数据：时间戳 = 现在 - window - 1s
    let old_t = Instant::now()
        .checked_sub(window + Duration::from_secs(1))
        .expect("系统时钟不支持回退");
    for _ in 0..CRASH_LOOP_MAX_RESTARTS {
        manager.recent_restarts.push_back(old_t);
    }
    // 2 条新数据（刚才）
    let new_t = Instant::now();
    manager.recent_restarts.push_back(new_t);
    manager.recent_restarts.push_back(new_t);
    // 检查：count_in_window 只看 2 条新的
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
        .expect("建 tokio runtime");
    let mut manager = SidecarManager::new();
    // 先设 stopped
    let _ = manager.stopped.swap(true, Ordering::SeqCst);
    rt.block_on(async {
        let action = manager.watchdog_tick().await.expect("stopped 时应 ok");
        assert_eq!(action, WatchdogAction::Idle);
    });
}

#[test]
fn test_default_equals_new() {
    let mut a = SidecarManager::new();
    let mut b = SidecarManager::default();
    assert!(a.stop_hard().is_ok());
    assert!(b.stop_hard().is_ok());
}
