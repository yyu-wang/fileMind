//! 推理模式切换安全阀：自动切换云端必须先获得用户同意。

use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicBool, Ordering};

static CONSENT_GIVEN: AtomicBool = AtomicBool::new(false);

/// 设置用户是否同意数据上云。
pub fn set_consent(given: bool) {
    CONSENT_GIVEN.store(given, Ordering::SeqCst);
}

/// 查询用户是否已同意上云。
#[must_use]
pub fn has_consent() -> bool {
    CONSENT_GIVEN.load(Ordering::SeqCst)
}

/// 校验一次模式切换是否被允许。
///
/// # Errors
///
/// 以下情形返回 `ModeSwitchForbidden`：自动切换但无用户同意、
/// 目标与当前模式相同、目标为云端但无用户同意。
pub fn validate_mode_switch(current: &str, target: &str, source: &str) -> AppResult<()> {
    if source == "auto" && !has_consent() {
        return Err(AppError::ModeSwitchForbidden {
            current: current.to_string(),
        });
    }

    if current == target {
        return Err(AppError::ModeSwitchForbidden {
            current: current.to_string(),
        });
    }

    if target == "cloud" && !has_consent() {
        return Err(AppError::ModeSwitchForbidden {
            current: current.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// BE-m5：测试并行共享 static `CONSENT_GIVEN` 会互相污染（且原恢复在断言后，
    /// 失败时泄漏状态到下一测）；模块级锁强制串行 + 每测开头显式复位，
    /// 正确性不再依赖收尾时机。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_switch_without_consent_fails() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_consent(false);
        let result = validate_mode_switch("local", "cloud", "auto");
        assert!(result.is_err());
    }

    #[test]
    fn test_switch_with_consent_succeeds() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_consent(true);
        let result = validate_mode_switch("local", "cloud", "auto");
        assert!(result.is_ok());
        set_consent(false);
    }

    #[test]
    fn test_same_mode_fails() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        set_consent(true);
        let result = validate_mode_switch("local", "local", "auto");
        assert!(result.is_err());
    }
}
