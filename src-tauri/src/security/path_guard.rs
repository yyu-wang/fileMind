//! 路径安全守卫：规范化路径并拦截系统目录黑名单与越界访问。

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

/// 校验读路径：规范化并检查黑名单，返回规范化路径。
///
/// # Errors
///
/// 路径不存在、无法解析或命中黑名单时返回 `UnsafePath`。
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

    Ok(canonical)
}

/// 校验路径必须位于指定根目录内（防越界）。
///
/// # Errors
///
/// 同 [`validate`]；此外路径不在根目录内时返回 `UnsafePath`。
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

/// 校验相对子路径（如分类目标目录 `target_dir`）：只允许正常相对分段。
///
/// 拒绝：空串、绝对路径、Windows 盘符前缀、反斜杠、`.`/`..` 等特殊分段。
/// 返回未规范化的 `PathBuf`（调用方负责与扫描根拼接）。
///
/// # Errors
///
/// 任一非法形式命中时返回 `UnsafePath`。
pub fn validate_relative_subpath(relative: &str) -> AppResult<PathBuf> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(AppError::UnsafePath("相对子路径为空".to_string()));
    }
    if Path::new(trimmed).is_absolute() {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能是绝对路径: {relative}"
        )));
    }
    if trimmed.contains('\\') {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能包含反斜杠: {relative}"
        )));
    }
    // Windows 盘符前缀（C: / C:/x）—— 防御跨平台路径注入
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(AppError::UnsafePath(format!(
            "相对子路径不能包含盘符: {relative}"
        )));
    }
    for component in Path::new(trimmed).components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(AppError::UnsafePath(format!(
                "相对子路径含非法分段: {relative}"
            )));
        }
    }
    Ok(Path::new(trimmed).to_path_buf())
}

/// 校验写目标路径（移动/重命名）：目标本身可不存在。
///
/// 逐级检查路径字符串黑名单，并校验已存在的父目录。
///
/// # Errors
///
/// 目标为空、命中黑名单或父目录无法解析时返回 `UnsafePath`。
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
    fn test_validate_write_target_blocked_parent() {
        let result = validate_write_target("/usr/local/new_file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_relative_subpath_valid() -> Result<(), Box<dyn std::error::Error>> {
        let path = validate_relative_subpath("图片/报表")?;
        assert_eq!(path.to_string_lossy(), "图片/报表");
        Ok(())
    }

    #[test]
    fn test_validate_relative_subpath_empty_rejected() {
        assert!(validate_relative_subpath("").is_err());
        assert!(validate_relative_subpath("   ").is_err());
    }

    #[test]
    fn test_validate_relative_subpath_absolute_rejected() {
        assert!(validate_relative_subpath("/etc").is_err());
        assert!(validate_relative_subpath("C:/windows").is_err());
    }

    #[test]
    fn test_validate_relative_subpath_dot_segments_rejected() {
        assert!(validate_relative_subpath("..").is_err());
        assert!(validate_relative_subpath("../图片").is_err());
        assert!(validate_relative_subpath("图片/../secret").is_err());
        assert!(validate_relative_subpath("./图片").is_err());
    }

    #[test]
    fn test_validate_relative_subpath_backslash_rejected() {
        assert!(validate_relative_subpath("图片\\报表").is_err());
    }

    #[test]
    fn test_validate_relative_subpath_drive_prefix_rejected() {
        assert!(validate_relative_subpath("C:").is_err());
        assert!(validate_relative_subpath("D:/x").is_err());
    }
}
