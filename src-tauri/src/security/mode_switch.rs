use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicBool, Ordering};

static CONSENT_GIVEN: AtomicBool = AtomicBool::new(false);

pub fn set_consent(given: bool) {
    CONSENT_GIVEN.store(given, Ordering::SeqCst);
}

pub fn has_consent() -> bool {
    CONSENT_GIVEN.load(Ordering::SeqCst)
}

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

    #[test]
    fn test_switch_without_consent_fails() {
        set_consent(false);
        let result = validate_mode_switch("local", "cloud", "auto");
        assert!(result.is_err());
    }

    #[test]
    fn test_switch_with_consent_succeeds() {
        set_consent(true);
        let result = validate_mode_switch("local", "cloud", "auto");
        assert!(result.is_ok());
        set_consent(false);
    }

    #[test]
    fn test_same_mode_fails() {
        set_consent(true);
        let result = validate_mode_switch("local", "local", "auto");
        assert!(result.is_err());
    }
}
