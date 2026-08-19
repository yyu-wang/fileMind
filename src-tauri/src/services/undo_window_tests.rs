//! T3.6 — `undo_window::is_within_window` 单元测试。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Duration, Utc};

use crate::services::undo_window::is_within_window;

#[test]
fn test_within_window_now() {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let r = is_within_window(&now).unwrap();
    assert!(r, "当前时间应在窗口内");
}

#[test]
fn test_within_window_23h_ago() {
    let t = (Utc::now() - Duration::hours(23))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let r = is_within_window(&t).unwrap();
    assert!(r, "23 小时前应在窗口内");
}

#[test]
fn test_outside_window_25h_ago() {
    let t = (Utc::now() - Duration::hours(25))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let r = is_within_window(&t).unwrap();
    assert!(!r, "25 小时前应超出窗口");
}

#[test]
fn test_window_boundary_24h_ago() {
    // 刚好 24 小时 → 严格小于窗口期才允许，应返回 false
    let t = (Utc::now() - Duration::hours(24))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let r = is_within_window(&t).unwrap();
    assert!(!r, "刚好 24 小时应超出窗口（严格小于）");
}

#[test]
fn test_invalid_format_returns_err() {
    let r = is_within_window("not-a-date");
    assert!(r.is_err(), "非法格式应返回 Err");
}
