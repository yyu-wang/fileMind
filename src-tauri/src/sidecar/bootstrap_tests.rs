//! `sidecar::bootstrap` 单元测试（子模块 `super::*` 可访问私有函数）。
//!
//! 独立文件拆分原因：`bootstrap.rs` 主实现内嵌 tests 模块会超 Rust 模块 300 行阈值
//! （与 `manager.rs` / `manager_tests.rs` 的拆分口径一致）。

#![allow(clippy::expect_used)]
// 测试代码允许 expect：表达「这条路径必须成功，否则测试直接挂掉」是最直观的语义。

use std::path::{Path, PathBuf};

use super::*;

/// 构造临时「可执行文件」目录，返回 (dir, env, hint, bundle) 路径。
fn make_files() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir 创建失败");
    let env = dir.path().join("env");
    let hint = dir.path().join("dev");
    let bundle = dir.path().join("bundle");
    std::fs::write(&env, b"x").expect("写入 env 文件失败");
    std::fs::write(&hint, b"x").expect("写入 hint 文件失败");
    std::fs::write(&bundle, b"x").expect("写入 bundle 文件失败");
    (dir, env, hint, bundle)
}

#[test]
fn pick_binary_prefers_env() {
    let (_dir, env, hint, bundle) = make_files();
    let picked = pick_binary_path(
        Some(env.to_str().expect("env 路径含非 UTF-8")),
        &hint,
        Some(bundle),
    )
    .expect("env 命中应成功");
    assert_eq!(picked, env);
}

#[test]
fn pick_binary_env_dir_uses_main_exe_inside() {
    // P2-2：env 覆盖指向 onedir 目录 → 取目录内主可执行
    let dir = tempfile::tempdir().expect("tempdir 创建失败");
    let prod = dir.path().join("filemind-sidecar");
    std::fs::create_dir_all(&prod).expect("mkdir onedir 失败");
    let exe = prod.join("filemind-sidecar");
    std::fs::write(&exe, b"x").expect("写入主可执行失败");
    let picked = pick_binary_path(
        Some(prod.to_str().expect("路径含非 UTF-8")),
        Path::new(""),
        None,
    )
    .expect("override 指向 onedir 目录应成功");
    assert_eq!(picked, exe);
}

#[test]
fn pick_binary_env_missing_is_error() {
    let (dir, _env, hint, bundle) = make_files();
    let missing = dir.path().join("missing");
    let err = pick_binary_path(
        Some(missing.to_str().expect("路径含非 UTF-8")),
        &hint,
        Some(bundle),
    )
    .expect_err("env 指定但不存在的文件应报错");
    assert!(err.to_string().contains("FILEMIND_SIDECAR_BINARY"));
}

#[test]
fn pick_binary_hint_wins_over_bundle() {
    let (_dir, _env, hint, bundle) = make_files();
    let picked = pick_binary_path(None, &hint, Some(bundle)).expect("hint 命中应成功");
    assert_eq!(picked, hint);
}

#[test]
fn pick_binary_falls_back_to_bundle() {
    let dir = tempfile::tempdir().expect("tempdir 创建失败");
    let bundle = dir.path().join("bundle");
    std::fs::write(&bundle, b"x").expect("写入 bundle 文件失败");
    let picked =
        pick_binary_path(None, Path::new(""), Some(bundle.clone())).expect("bundle 应命中");
    assert_eq!(picked, bundle);
}

#[test]
fn pick_binary_none_hits_is_error() {
    let dir = tempfile::tempdir().expect("tempdir 创建失败");
    let err = pick_binary_path(None, Path::new(""), Some(dir.path().join("none")))
        .expect_err("无任何命中应报错");
    assert!(err.to_string().contains("未找到 Sidecar 二进制"));
}
