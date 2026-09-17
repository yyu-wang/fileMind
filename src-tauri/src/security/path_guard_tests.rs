//! `path_guard` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `security/keychain_tests.rs`、`security/cloud_proxy_tests.rs`）。

// 测试模块允许 expect/unwrap（项目惯例）
#![allow(clippy::unwrap_used, clippy::expect_used)]

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

    let result = validate_within_root(tmp2.path().to_str().ok_or("non-UTF8 path")?, tmp1.path());
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

// BE-M4 核心回归：目标与父目录都不存在 + 中间符号链接指向黑名单目录。
// 旧实现只做字符串前缀检查，/tmp/link/newdir/f.txt 可绕过；
// 新实现 canonicalize 最近存在祖先（/tmp/link → /etc）后拦截。
#[test]
fn test_validate_write_target_symlink_bypass_blocked() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink("/etc", &link)?;
    // target 与其父目录 newdir 均不存在，只有 link 存在
    let target = link.join("newdir").join("f.txt");
    assert!(validate_write_target(target.to_str().expect("UTF-8 path")).is_err());
    Ok(())
}

// macOS 大小写不敏感文件系统：/system 与 /System 是同一路径，字符串检查必须命中。
#[test]
fn test_blocked_case_insensitive() {
    assert!(validate_write_target("/system/library/x").is_err());
    assert!(validate_write_target("/ETC/hosts").is_err());
    assert!(validate("/Applications").is_err());
}

// BE-M8：扩充黑名单逐项验证。
#[test]
fn test_blocked_expanded_patterns() {
    for p in [
        "/etc/hosts",
        "/private/etc/hosts",
        "/Applications/App.app",
        "/opt/homebrew/bin/tool",
        "/System/Library/CoreServices/x",
    ] {
        assert!(validate_write_target(p).is_err(), "{p} 应被拦截");
    }
}

// BE-M8：每用户系统目录（Library / .ssh）。
#[test]
fn test_blocked_per_user_paths() {
    assert!(validate_write_target("/Users/someone/Library/Preferences/x.plist").is_err());
    assert!(validate_write_target("/Users/someone/.ssh/config").is_err());
    assert!(validate_write_target("/home/dev/.ssh/authorized_keys").is_err());
    // Linux 家目录下的 Library 是用户内容，不拦截。
    // macOS 上 /home 是指向 /System/Volumes/Data/home 的符号链接，
    // canonicalize 后命中 /system 前缀被拦——属预期安全行为，故仅 Linux 断言。
    #[cfg(target_os = "linux")]
    assert!(validate_write_target("/home/dev/Library/books").is_ok());
}

// 段边界对齐：相近前缀名不得误伤。
#[test]
fn test_no_false_positive_on_similar_names() {
    assert!(validate_write_target("/etcetera/notes").is_ok());
    assert!(validate_write_target("/usage.txt").is_ok());
    assert!(validate_write_target("/binary/data").is_ok());
}

// 正常多级新建目录（祖先全不存在）仍放行。
#[test]
fn test_validate_write_target_deep_new_dirs_ok() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("a").join("b").join("c.txt");
    let result = validate_write_target(target.to_str().ok_or("non-UTF8 path")?);
    assert!(result.is_ok());
    Ok(())
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
