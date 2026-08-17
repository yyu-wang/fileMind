use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

const BLOCKED_PATTERNS: &[&str] = &[
    "/System",
    "/Library",
    "/usr",
    "/bin",
    "/sbin",
    "/dev",
    "/proc",
    "/sys",
    "C:\\Windows\\System32",
    "C:\\Program Files",
];

pub fn validate(path: &str) -> AppResult<PathBuf> {
    let canonical = Path::new(path)
        .canonicalize()
        .map_err(|_| AppError::UnsafePath(format!("路径不存在或无法解析: {path}")))?;

    for pattern in BLOCKED_PATTERNS {
        if canonical.starts_with(pattern) {
            return Err(AppError::UnsafePath(format!(
                "路径被安全策略阻止: {path} 匹配黑名单 {pattern}"
            )));
        }
    }

    if canonical.is_dir() && !canonical.exists() {
        return Err(AppError::UnsafePath(format!("目录不存在: {path}")));
    }

    Ok(canonical)
}

pub fn validate_within_root(path: &str, root: &Path) -> AppResult<PathBuf> {
    let canonical = validate(path)?;
    let root_canonical = root
        .canonicalize()
        .map_err(|_| AppError::UnsafePath(format!("根目录无法解析: {}", root.display())))?;

    if !canonical.starts_with(&root_canonical) {
        return Err(AppError::UnsafePath(format!(
            "路径越界: {} 不在根目录 {} 内",
            canonical.display(),
            root_canonical.display()
        )));
    }

    Ok(canonical)
}

/// Validates a target path for write operations (move/rename).
/// Unlike `validate`, this does not require the path to exist.
/// Checks the path string against blocked patterns and validates
/// the parent directory if it exists.
pub fn validate_write_target(target: &str) -> AppResult<PathBuf> {
    if target.is_empty() {
        return Err(AppError::UnsafePath("目标路径为空".to_string()));
    }

    let target_path = Path::new(target);

    check_blocked_patterns(&target_path.to_string_lossy(), target_path)?;

    if target_path.exists() {
        let canonical = target_path
            .canonicalize()
            .map_err(|_| AppError::UnsafePath(format!("目标路径无法解析: {target}")))?;
        check_blocked_patterns_canonical(target, &canonical)?;
    } else if let Some(parent) = target_path.parent() {
        if parent.exists() {
            let parent_canonical = parent.canonicalize().map_err(|_| {
                AppError::UnsafePath(format!("目标父目录无法解析: {}", parent.display()))
            })?;
            check_blocked_patterns_canonical(&parent.display().to_string(), &parent_canonical)?;
        }
    }

    Ok(target_path.to_path_buf())
}

fn check_blocked_patterns(display: &str, path: &Path) -> AppResult<()> {
    for pattern in BLOCKED_PATTERNS {
        if path.starts_with(pattern) {
            return Err(AppError::UnsafePath(format!(
                "路径被安全策略阻止: {display} 匹配黑名单 {pattern}"
            )));
        }
    }
    Ok(())
}

fn check_blocked_patterns_canonical(display: &str, canonical: &Path) -> AppResult<()> {
    for pattern in BLOCKED_PATTERNS {
        if canonical.starts_with(pattern) {
            return Err(AppError::UnsafePath(format!(
                "路径被安全策略阻止: {display} 匹配黑名单 {pattern}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_valid_path() -> Result<(), Box<dyn std::error::Error>> {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let result = validate(&home);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_blocked_system_path() {
        let result = validate("/System");
        assert!(result.is_err());
    }

    #[test]
    fn test_blocked_proc_path() {
        let result = validate("/proc");
        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_path() {
        let result = validate("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_within_root() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().to_path_buf();
        let sub = root.join("subdir");
        std::fs::create_dir(&sub)?;

        let result = validate_within_root(sub.to_str().ok_or("non-UTF8 path")?, &root);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_path_outside_root() -> Result<(), Box<dyn std::error::Error>> {
        let tmp1 = tempfile::tempdir()?;
        let tmp2 = tempfile::tempdir()?;

        let result =
            validate_within_root(tmp2.path().to_str().ok_or("non-UTF8 path")?, tmp1.path());
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_validate_write_target_empty() {
        let result = validate_write_target("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_write_target_blocked_system() {
        let result = validate_write_target("/System/evil.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_write_target_valid() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("new_file.txt");
        let result = validate_write_target(target.to_str().ok_or("non-UTF8 path")?);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_validate_write_target_blocked_parent() -> Result<(), Box<dyn std::error::Error>> {
        let result = validate_write_target("/usr/local/new_file.txt");
        assert!(result.is_err());
        Ok(())
    }
}
