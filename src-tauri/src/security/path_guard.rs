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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_valid_path() {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let result = validate(&home);
        assert!(result.is_ok());
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
    fn test_path_within_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let sub = root.join("subdir");
        std::fs::create_dir(&sub).unwrap();

        let result = validate_within_root(sub.to_str().unwrap(), &root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_outside_root() {
        let tmp1 = tempfile::tempdir().unwrap();
        let tmp2 = tempfile::tempdir().unwrap();

        let result = validate_within_root(
            tmp2.path().to_str().unwrap(),
            tmp1.path(),
        );
        assert!(result.is_err());
    }
}
