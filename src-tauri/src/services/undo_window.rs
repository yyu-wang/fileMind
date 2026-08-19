//! 撤销窗口判断：批次创建时间在窗口期内才允许撤销。
//!
//! 设计要点：
//! - 默认窗口期 24 小时（`UNDO_WINDOW_HOURS` 常量）
//! - 用 `operations_log.created_at`（TEXT ISO 8601 "YYYY-MM-DD HH:MM:SS" UTC）与当前 UTC 比较
//! - 超出窗口 → `can_undo=false`（即使 `status='done'`）
//! - 后续 T6.7 设置页会把窗口期暴露为可配置项，本任务先用常量
//!
//! 07-T-02 缓解措施要求：操作日志支持撤销，但需限制时间窗口防止误操作回滚已整理状态。

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::error::{AppError, AppResult};

/// 撤销窗口期（小时）。批次创建时间超出此值则不可撤销。
pub const UNDO_WINDOW_HOURS: i64 = 24;

/// 判断批次是否在撤销窗口内。
///
/// `batch_created_at` 格式：`YYYY-MM-DD HH:MM:SS`（UTC，与 `operations_log.created_at` 一致）。
/// 返回 `true` 表示在窗口内（可撤销）；`false` 表示超出窗口。
///
/// 边界：刚好 `UNDO_WINDOW_HOURS` 小时前返回 `false`（严格小于窗口期才允许撤销）。
///
/// # Errors
///
/// 时间解析失败返回 `AppError::InvalidInput`。
pub fn is_within_window(batch_created_at: &str) -> AppResult<bool> {
    let batch_time = parse_sqlite_datetime(batch_created_at)?;
    let now = Utc::now();
    let elapsed = now - batch_time;
    Ok(elapsed.num_hours() < UNDO_WINDOW_HOURS)
}

/// 解析 `SQLite` TEXT 格式时间戳为 UTC `DateTime`。
fn parse_sqlite_datetime(s: &str) -> AppResult<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc())
        .map_err(|e| AppError::InvalidInput(format!("解析时间失败: {e}")))
}
