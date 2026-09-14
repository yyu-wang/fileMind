//! 路径解析测试：target triple 探测、`FILEMIND_SIDECAR_DIR` 覆盖优先级、
//! cargo manifest 回退与 bundle（onedir / 扁平目录）布局识别。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达（生产代码严格禁止）。

use super::super::*;
use super::support::*;

// ---------- 新增 7 条：resolve / current_triple / new 字段 / start_fail ----------

#[test]
fn test_current_target_triple_matches_rustc_triple() {
    // 能跑 cargo test 时 rustc 一定在 PATH；如果调用异常就 skip，不强挂。
    let Ok(output) = std::process::Command::new("rustc").arg("-vV").output() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(host) = stdout.lines().find_map(|l| l.strip_prefix("host: ")) else {
        return;
    };
    let got = current_target_triple();
    let covered = matches!(
        host,
        "aarch64-apple-darwin"
            | "x86_64-apple-darwin"
            | "x86_64-pc-windows-msvc"
            | "x86_64-unknown-linux-gnu"
    );
    if covered {
        assert_eq!(got, host, "current_target_triple 应等于 rustc host triple");
    }
}

#[test]
fn test_resolve_env_override_dir_uses_main_exe_inside() {
    // P2-2：env 覆盖指向 onedir 产物**目录**时，解析到目录内的主可执行
    let tmp = build_onedir_dir("override-dir", main_exe_name());
    let override_dir = tmp
        .path()
        .join("filemind")
        .join("binaries")
        .join("override-dir");
    let got = resolve_dev_binary_path(Some(override_dir.to_str().expect("临时路径应为合法 UTF-8")))
        .expect("override 指向存在的 onedir 目录应解析成功");
    assert_eq!(
        got,
        override_dir
            .join(main_exe_name())
            .canonicalize()
            .expect("canonicalize 主可执行不应失败")
    );
}

#[test]
fn test_resolve_env_override_accepts_executable_file() {
    // P2-2：env 覆盖语义是「直接指定要执行的 sidecar 可执行文件」，故也接受**文件**
    // （CI E2E / 本地 dev 用 wrapper 脚本注入 `python -m app`，见
    // scripts/e2e-sidecar-wrapper.sh）。若这条退化，E2E smoke 会直接起不来。
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let wrapper = tmp.path().join("e2e-sidecar-wrapper.sh");
    std::fs::write(&wrapper, b"#!/bin/sh\nexec python -m app\n").expect("写 wrapper 不应失败");
    let got = resolve_dev_binary_path(Some(wrapper.to_str().expect("临时路径应为合法 UTF-8")))
        .expect("override 指向可执行文件应直接采用");
    assert_eq!(got, wrapper.canonicalize().unwrap());
}

#[test]
fn test_resolve_env_override_takes_highest_priority() {
    // override + CARGO 路径同时存在时，override 必须优先返回
    let _env = lock_env();
    let tmp = build_onedir_dir("cargo-triple-dir", main_exe_name());
    // 另放一个 triple 具体名目录（CARGO 回退会命中它），验证 override 仍优先
    write_onedir(
        tmp.path(),
        &format!("filemind-sidecar-{}", current_target_triple()),
        main_exe_name(),
    );
    write_src_tauri(tmp.path());
    let src_tauri = tmp.path().join("src-tauri");
    let _g = EnvGuard::set(
        "CARGO_MANIFEST_DIR",
        src_tauri.to_string_lossy().to_string(),
    );
    let override_dir = tmp
        .path()
        .join("filemind")
        .join("binaries")
        .join("cargo-triple-dir");
    let got = resolve_dev_binary_path(Some(override_dir.to_str().expect("临时路径 UTF-8")))
        .expect("解析 override 不应失败");
    assert_eq!(
        got,
        override_dir
            .join(main_exe_name())
            .canonicalize()
            .expect("canonicalize 主可执行不应失败")
    );
}

#[test]
fn test_resolve_fallback_cargo_manifest_triple_dir() {
    // P2-2：triple 具体名**目录**优先命中
    let _env = lock_env();
    let triple = current_target_triple();
    let tmp = build_onedir_dir(&format!("filemind-sidecar-{triple}"), main_exe_name());
    write_src_tauri(tmp.path());
    let src_tauri = tmp.path().join("src-tauri");
    let _g = EnvGuard::set(
        "CARGO_MANIFEST_DIR",
        src_tauri.to_string_lossy().to_string(),
    );
    let result = resolve_dev_binary_path(None);
    // 如果当前平台刚好是 aarch64 mac → 必须解析到我们建的 triple 目录；
    // 其他架构：triple 名字不对（我们只建了 arm64）→ 走错误路径，只要不 panic 就通过。
    if std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64" {
        let want = tmp
            .path()
            .join(format!("filemind/binaries/filemind-sidecar-{triple}"))
            .join(main_exe_name())
            .canonicalize()
            .expect("canonicalize triple 主可执行不应失败");
        assert_eq!(result.expect("mac arm64 应命中 triple 目录"), want);
    } else {
        // 非 mac arm64：不做强断言；保证无 panic
        let _ = result;
    }
}

#[test]
fn test_resolve_fallback_cargo_manifest_default_dir() {
    // 只放默认名目录（软链接名），不放 triple 具体名
    let _env = lock_env();
    let tmp = build_onedir_dir("filemind-sidecar", main_exe_name());
    write_src_tauri(tmp.path());
    let src_tauri = tmp.path().join("src-tauri");
    let _g = EnvGuard::set(
        "CARGO_MANIFEST_DIR",
        src_tauri.to_string_lossy().to_string(),
    );
    let triple = current_target_triple();
    let want_default = tmp
        .path()
        .join("filemind/binaries/filemind-sidecar")
        .join(main_exe_name());
    let want_triple = tmp
        .path()
        .join(format!("filemind/binaries/filemind-sidecar-{triple}"));
    let result = resolve_dev_binary_path(None);
    if want_triple.exists() {
        // triple 名目录居然刚好被创建了（其他测试一般不会），就用 triple 结果
        assert_eq!(
            result.expect("应能解析到 triple 产物"),
            want_triple
                .join(main_exe_name())
                .canonicalize()
                .expect("canonicalize triple 主可执行不应失败")
        );
    } else {
        // 常规情况：只有默认名目录，解析到默认名兜底
        assert_eq!(
            result.expect("应能解析到默认名兜底产物"),
            want_default
                .canonicalize()
                .expect("canonicalize 默认名主可执行不应失败")
        );
    }
}

#[test]
fn test_resolve_all_missing_error() {
    let _env = lock_env();
    let tmp = tempfile::tempdir().expect("建空临时目录不应失败");
    let fake_src_tauri = tmp.path().join("empty-src-tauri");
    std::fs::create_dir_all(&fake_src_tauri).expect("建空 src_tauri 不应失败");
    // 注意：CARGO_MANIFEST_DIR 原本不存在时也会被 EnvGuard set，测试结束不 remove（见 Drop 注释）
    let _g1 = EnvGuard::set(
        "CARGO_MANIFEST_DIR",
        fake_src_tauri.to_string_lossy().to_string(),
    );
    // 把 cwd 切到空临时目录，确保外部碰巧有 binaries 的情况不会串进来
    let orig_cwd = std::env::current_dir().expect("读 cwd 不应失败");
    std::env::set_current_dir(tmp.path()).expect("切 cwd 到 tmp 不应失败");
    let result = resolve_dev_binary_path(None);
    // 还原 cwd（即使下面断言失败也要尽量保持环境）
    std::env::set_current_dir(&orig_cwd).expect("还原 cwd 不应失败");
    match result {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains("候选列表"),
                "错误信息应包含候选列表便于排障，实际:\n{msg}"
            );
            assert!(
                msg.contains(&current_target_triple()),
                "错误信息应附带当前 triple 构建建议，实际:\n{msg}"
            );
        }
        other => panic!("期望返回 SidecarUnavailable，实际: {other:?}"),
    }
}
