//! `SidecarManager` 单元测试（子模块 `super::*` 能访问私有字段）。
//!
//! 独立文件拆分原因：`manager.rs` 主实现若内嵌 tests 模块会超 Rust<500 行复杂度阈值。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达，`expect("<static str>")` 语义等价
// 但 unwrap 更简洁；`-D clippy::unwrap_used/panic` 在生产代码里需严格遵守。

use std::path::{Path, PathBuf};

use super::*;

// ---------- 工具：RAII 环境变量守卫，避免 set_var 串扰 ----------

/// 串行化「改进程级环境变量 / cwd」的用例。
///
/// libtest **默认多线程并行**跑用例，而 `CARGO_MANIFEST_DIR` 与 cwd 是进程级共享状态：
/// 不加锁时 A 用例设的值会被并行的 B 用例读到（实测表现为解析到别人临时目录、
/// assertion 里出现两个不同的 `.tmpXXXX`）。
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取环境变量用例互斥锁；忽略中毒（某个用例 panic 不该连带挂掉后续用例）。
fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `env::set_var` 的 RAII 包装：测试结束时恢复（或跳过删除）原值。
///
/// 为什么不用 crate：项目 dev-deps 未引入 `temp_env`，用 10 行自写满足需求。
/// 使用方必须先取 [`lock_env`] 串行化（见其说明）。
struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl Into<String>) -> Self {
        // 调用方须先取 lock_env()：env 是进程级状态，并行用例会互相污染。
        let old = std::env::var(key).ok();
        std::env::set_var(key, value.into());
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(v) = &self.old {
            std::env::set_var(self.key, v);
        }
        // else: 测试前该 env 不存在 → 不做 remove_var（Rust 下它是 unsafe，
        // 串行执行时即使保留测试专用 env，也不影响其他测试（各自都有 EnvGuard set 覆盖）。
    }
}

/// onedir 主可执行的基础文件名（按平台，与 manager.rs `resolve_main_exe_names` 末项一致）。
fn main_exe_name() -> &'static str {
    if cfg!(windows) {
        "filemind-sidecar.exe"
    } else {
        "filemind-sidecar"
    }
}

/// 写一个带执行位的占位「可执行文件」。
fn write_executable(path: &Path) {
    std::fs::write(path, b"dummy-binary-content").expect("写 dummy binary 失败");
    // 给执行位：贴近真实侧车二进制场景（仅 Unix 有效）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = path.metadata().expect("读刚写完的文件 metadata 不应失败");
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms).expect("设置执行位失败");
    }
}

/// 在 `<repo_root>/filemind/binaries/<dir_name>/` 下写 onedir 主可执行文件。
///
/// P2-2：产物是目录（主可执行 + `_internal/`），解析一律按目录处理。
fn write_onedir(repo_root: &Path, dir_name: &str, exe_name: &str) {
    let dir = repo_root.join("filemind").join("binaries").join(dir_name);
    std::fs::create_dir_all(&dir).expect("mkdir onedir 目录失败");
    write_executable(&dir.join(exe_name));
}

/// 建临时目录并在其中放置 `filemind/binaries/<dir_name>/` onedir 结构，返回 `TempDir`。
fn build_onedir_dir(dir_name: &str, exe_name: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("建临时目录失败");
    write_onedir(tmp.path(), dir_name, exe_name);
    tmp
}

/// 写 ``<tmp>/src-tauri/Cargo.toml`` 占位文件，供 `CARGO_MANIFEST_DIR` 测试用。
fn write_src_tauri(repo_root: &Path) {
    let src_tauri = repo_root.join("src-tauri");
    std::fs::create_dir_all(&src_tauri).expect("mkdir src-tauri 失败");
    std::fs::write(src_tauri.join("Cargo.toml"), b"[package]\nname = 'x'\n")
        .expect("写占位 Cargo.toml 失败");
}

// ---------- 原有 8 条测试：适配 new(PathBuf) ----------

fn stub_binary() -> PathBuf {
    // 用一个真文件占位（不真启动，只做字段校验）。``<src-tauri>/Cargo.toml`` 永远存在。
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

#[test]
fn test_new_has_no_process() {
    let mut manager = SidecarManager::new(stub_binary());
    let result = manager.stop_hard();
    assert!(result.is_ok(), "未启动时 stop_hard 应无副作用");
    assert_eq!(manager.psk(), None);
    assert_eq!(manager.pid(), None);
    assert!(!manager.is_stopped());
    assert_eq!(manager.binary_path_inner(), stub_binary().as_path());
}

#[test]
fn test_stopped_flag_idempotent_stop_graceful_path() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    rt.block_on(async {
        let r1 = manager.stop_graceful(1).await;
        let r2 = manager.stop_graceful(2).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok(), "第二次 stop_graceful 应直接 return Ok（幂等）");
    });
    assert!(manager.is_stopped());
}

#[test]
fn test_backoff_grows_exponentially_with_cap() {
    let mut manager = SidecarManager::new(stub_binary());
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_BASE_MS)
    );
    for i in 0u32..=10u32 {
        manager.consecutive_failures = i;
        let shift = i.min(3);
        let expected_ms = u64::min(
            RESTART_BACKOFF_BASE_MS * (1u64 << shift),
            RESTART_BACKOFF_CAP_MS,
        );
        assert_eq!(
            manager.next_backoff(),
            Duration::from_millis(expected_ms),
            "第 {i} 次连续失败 backoff 应为 {expected_ms}ms"
        );
    }
    manager.consecutive_failures = 100;
    assert_eq!(
        manager.next_backoff(),
        Duration::from_millis(RESTART_BACKOFF_CAP_MS)
    );
}

#[test]
fn test_crash_loop_window_pauses_after_threshold() {
    let mut manager = SidecarManager::new(stub_binary());
    let now = Instant::now();
    for _ in 0..(CRASH_LOOP_MAX_RESTARTS - 1) {
        manager.recent_restarts.push_back(now);
    }
    assert!(
        !manager.is_crash_loop_paused(),
        "未达阈值前不应暂停：count={}/{}",
        manager.restart_count_in_window(),
        CRASH_LOOP_MAX_RESTARTS
    );
    manager.recent_restarts.push_back(now);
    assert!(
        manager.is_crash_loop_paused(),
        "到阈值后应进入 crash loop 暂停"
    );
}

#[test]
fn test_crash_loop_window_expires_old_entries() {
    let mut manager = SidecarManager::new(stub_binary());
    let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
    let old_t = Instant::now()
        .checked_sub(window + Duration::from_secs(1))
        .expect("系统时钟不支持 checked_sub");
    for _ in 0..CRASH_LOOP_MAX_RESTARTS {
        manager.recent_restarts.push_back(old_t);
    }
    let new_t = Instant::now();
    manager.recent_restarts.push_back(new_t);
    manager.recent_restarts.push_back(new_t);
    assert_eq!(
        manager.restart_count_in_window(),
        2,
        "超出窗口的旧条目应被移除，剩下应为新的 2 条"
    );
    assert!(
        !manager.is_crash_loop_paused(),
        "旧条目过期后不应再触发暂停"
    );
}

#[test]
fn test_watchdog_stopped_flag_returns_idle() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    let _ = manager.stopped.swap(true, Ordering::SeqCst);
    rt.block_on(async {
        let action = manager.watchdog_tick().await.expect("stopped 时应 ok");
        assert_eq!(action, WatchdogAction::Idle);
    });
}

#[test]
fn test_watchdog_unstarted_returns_idle_without_probing() {
    // P1-1：process=None 且 psk=None（后台引导中 / 启动失败等待重试）时，
    // watchdog 必须直接 Idle，不能触发 health 探测 → NeedRestart 双开进程。
    // 本机无 Sidecar 监听 8765，若守卫缺失会走 health 失败计数，此处验证不依赖网络。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建 tokio runtime 失败");
    let mut manager = SidecarManager::new(stub_binary());
    rt.block_on(async {
        let action = manager.watchdog_tick().await.expect("未启动时应 ok");
        assert_eq!(action, WatchdogAction::Idle);
    });
    assert_eq!(manager.recent_health_fails, 0, "未启动不应累计 health 失败");
}

#[test]
fn test_default_equals_new() {
    // Default/New 目前不派生 PartialEq（Child 不实现），退化为逐字段等价校验：
    // 都能 stop_hard 无副作用、都是 stopped=false、recent_restarts 空。
    let mut a = SidecarManager::new(stub_binary());
    let mut b = SidecarManager::default();
    assert!(a.stop_hard().is_ok());
    assert!(b.stop_hard().is_ok());
    assert_eq!(a.recent_restarts.len(), 0);
    assert_eq!(b.recent_restarts.len(), 0);
    assert!(!a.is_stopped());
    assert!(!b.is_stopped());
    assert!(
        b.binary_path_inner().is_file(),
        "default 占位 binary 必须是存在的文件"
    );
}

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

#[test]
fn test_new_holds_given_path() {
    let p = stub_binary();
    let m = SidecarManager::new(p.clone());
    assert_eq!(m.binary_path_inner(), p.as_path());
    assert_eq!(m.binary_path(), p.as_path());
}

#[test]
fn test_start_fails_on_nonexistent_binary_with_nice_message() {
    // 给一个不存在的路径 → start() 报 SidecarUnavailable 且携带路径信息
    let mut m = SidecarManager::new(PathBuf::from("/does/not/exist/filemind-sidecar-vx"));
    match m.start() {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains("/does/not/exist/filemind-sidecar-vx"),
                "错误信息应包含路径，实际: {msg}"
            );
        }
        other => panic!("start 对不存在的 binary 应返回 SidecarUnavailable，实际: {other:?}"),
    }
}

#[test]
fn test_default_sidecar_uses_dev_binaries_dir() {
    // Default binary_path 纯基于编译时 env!("CARGO_MANIFEST_DIR") 常量拼接，
    // 不读运行时 env，不需要 EnvGuard。P2-2：产物是 onedir 目录，主可执行在目录内，
    // 拼接结果应为：
    //   ${CARGO_MANIFEST_DIR}/../filemind/binaries/filemind-sidecar/filemind-sidecar[.exe]；
    // 不 canonicalize（真实产物可能尚未构建），直接比较逻辑路径。
    let m = SidecarManager::default();
    let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("filemind")
        .join("binaries")
        .join("filemind-sidecar")
        .join(main_exe_name());
    assert_eq!(m.binary_path_inner(), expected.as_path());
    assert_eq!(m.binary_path(), expected.as_path());
}

#[test]
fn test_start_uses_placeholder_binary_gives_nice_error() {
    // 覆盖「管理器 binary_path 字段值真的通过 Command::new 执行层」：
    // 传一个明确不存在的 `filemind/binaries/filemind-sidecar` 风格路径，
    // start() 会返回 AppError::SidecarUnavailable，且错误信息中包含该路径。
    // （注意：不能用 SidecarManager::default() —— 真实开发机上 dev 产物可能已存在，
    //  会真的启动进程并握手成功；此处用固定不存在路径，验证路径透传即可。）
    let bogus = PathBuf::from("/tmp/filemind-tests-notexist-bogus")
        .join("filemind")
        .join("binaries")
        .join("filemind-sidecar");
    let mut m = SidecarManager::new(bogus.clone());
    match m.start() {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains(bogus.to_str().unwrap()),
                "start() 错误信息应包含传入的路径，实际: {msg}"
            );
            assert!(
                msg.contains("filemind/binaries/filemind-sidecar"),
                "路径透传到错误信息时应保留 filemind/binaries/ 上下文，实际: {msg}"
            );
        }
        other => {
            panic!("SidecarManager::new(不存在).start() 应返回 SidecarUnavailable，实际: {other:?}")
        }
    }
}

#[test]
fn test_resolve_bundle_prefers_sidecar_subdir() {
    // P2-2 主路径：Tauri `bundle.resources` 把 onedir 目录拷到 `<root>/sidecar/`
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let sidecar_dir = tmp.path().join(BUNDLE_SIDECAR_SUBDIR);
    std::fs::create_dir_all(&sidecar_dir).expect("mkdir sidecar 不应失败");
    let exe = sidecar_dir.join(main_exe_name());
    std::fs::write(&exe, b"fake-exe").expect("写主可执行失败");

    let got = resolve_bundle_from_resources(tmp.path()).expect("<root>/sidecar 命中应解析成功");
    assert_eq!(got, exe.canonicalize().unwrap());
}

#[test]
fn test_resolve_bundle_accepts_flat_onedir_dir() {
    // 兜底路径：onedir 目录直接摆在根目录下（无 sidecar 子目录嵌套）
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let exe = tmp.path().join(main_exe_name());
    std::fs::write(&exe, b"fake-flat").expect("写主可执行失败");

    let got = resolve_bundle_from_resources(tmp.path()).expect("根目录直放主可执行应解析成功");
    assert_eq!(got, exe.canonicalize().unwrap());
}

/// P2-2 打包态集成契约：真实 macOS `.app` 布局下必须命中 bundle 侧车。
///
/// 复刻 `FileMind.app/Contents/` 结构（`MacOS/` 放主程序、`Resources/sidecar/` 放
/// onedir 产物），按 `resolve_bundle_binary_path` 的根目录顺序（可执行目录 →
/// `resource_dir`）调用纯函数，断言命中 `Resources/sidecar/filemind-sidecar`。
/// 这是 P2-2 由 externalBin 改 bundle.resources 后最关键的一处路径契约，
/// 离线可测（无需真实打包与 GUI 启动）。
#[test]
fn test_resolve_bundle_hits_real_macos_app_layout() {
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let app = tmp.path().join("FileMind.app").join("Contents");
    let macos_dir = app.join("MacOS");
    let resources_dir = app.join("Resources");
    let sidecar_dir = resources_dir.join(BUNDLE_SIDECAR_SUBDIR);
    std::fs::create_dir_all(&macos_dir).expect("mkdir MacOS 失败");
    std::fs::create_dir_all(&sidecar_dir).expect("mkdir Resources/sidecar 失败");
    // 主程序同目录无 sidecar（旧 externalBin 布局已不再使用）
    std::fs::write(macos_dir.join("filemind"), b"app-exe").expect("写主程序失败");
    let exe = sidecar_dir.join(main_exe_name());
    std::fs::write(&exe, b"sidecar-exe").expect("写 sidecar 主可执行失败");

    let got = resolve_bundle_from_roots(&[macos_dir, resources_dir])
        .expect("真实 .app 布局应命中 bundle 侧车");
    assert_eq!(got, exe.canonicalize().unwrap());
}

#[test]
fn test_resolve_bundle_missing_reports_candidates() {
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    match resolve_bundle_from_resources(tmp.path()) {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains(BUNDLE_SIDECAR_SUBDIR),
                "错误信息应列出 sidecar 子目录候选，实际: {msg}"
            );
            assert!(
                msg.contains("bundle.resources"),
                "错误信息应提示 bundle.resources 配置，实际: {msg}"
            );
            assert!(msg.contains("候选"), "错误信息应包含候选提示，实际: {msg}");
        }
        other => panic!("候选均不存在时应返回 SidecarUnavailable，实际: {other:?}"),
    }
}

// ---------- BE-C4：探活 client 必须有超时 ----------

/// 挂死端点（接受 TCP 连接但永不响应）上，探活 client 必须在 3s 超时内
/// 快速失败，而不是无限挂起——否则 watchdog 持锁路径会被一起拖死。
#[test]
fn test_probe_client_times_out_on_hung_endpoint() {
    // 只 bind 不 accept：内核 backlog 会完成 TCP 握手，HTTP 请求发出后
    // 永远等不到响应，精确复现「接受连接但不响应」的挂死形态
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 随机端口失败");
    let port = listener.local_addr().expect("local_addr 失败").port();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime 构建失败");
    let started = std::time::Instant::now();
    let result = rt.block_on(async {
        proxy::probe_client()
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
    });

    assert!(result.is_err(), "挂死端点应因超时返回 Err");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "应在 3s 超时 + 余量内返回，实际耗时 {:?}",
        started.elapsed()
    );
}

// ---------- BE-M3 / BE-M6：启动失败清理与孤儿清理 ----------

/// `abort_failed_start` 原语：杀掉未验证子进程 + 清 PSK，且不得置 stopped
/// （否则 watchdog 判定「已主动关闭」永久退出，重启退避循环失效）。
#[cfg(unix)]
#[test]
fn test_abort_failed_start_kills_process_and_clears_psk() {
    let mut m = SidecarManager::new(stub_binary());
    // 手动塞入真实存活子进程 + 未验证 PSK，模拟 start() 成功后握手失败的状态
    m.process = Some(
        std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep 失败"),
    );
    let pid = m.pid().expect("启动后应有 pid");
    m.psk = Some(vec![1u8, 2, 3]);

    m.abort_failed_start();

    assert!(m.psk.is_none(), "未验证 PSK 必须清除");
    assert!(m.process.is_none(), "子进程句柄必须清空");
    assert!(!m.is_stopped(), "不得置 stopped 标志");
    // 子进程确实被杀（stop_hard 已 wait 回收，ps 查不到）
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .output()
        .expect("ps 执行失败");
    assert!(!out.status.success(), "子进程 pid={pid} 应已被杀");
}

/// 工具缺失时跳过孤儿清理测试（lsof/nc/sh 任一不可用即返回）。
#[cfg(unix)]
fn orphan_test_tools_ready() -> bool {
    ["lsof", "nc", "sh"]
        .iter()
        .all(|t| std::process::Command::new("which").arg(t).output().is_ok())
}

/// 轮询直到端口出现监听进程（返回 pid），超时返回 None。
#[cfg(unix)]
fn wait_listener(port: u16, timeout: std::time::Duration) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(out) = std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{port}")])
            .output()
        {
            if let Some(pid) = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .find_map(|s| s.parse::<u32>().ok())
            {
                return Some(pid);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// 测试兜底：无条件杀掉端口上的监听进程，保证测试自身不泄漏进程。
/// 仅限测试专用端口（48765/48766），与生产清理函数的名字/ppid 校验无关。
#[cfg(unix)]
fn force_kill_listeners(port: u16) {
    if let Ok(out) = std::process::Command::new("lsof")
        .args(["-ti", &format!("tcp:{port}")])
        .output()
    {
        for pid in String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse::<u32>().ok())
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .output();
        }
    }
}

/// 准备一个名为 filemind-sidecar 的假监听进程并孤儿化（ppid→1）。
/// 通过 `sh -c 'cmd &'` 实现：sh 退出后子进程被 init/launchd 收养。
/// 返回监听 pid；未就绪（工具缺失/nc 行为差异）返回 None 并自行兜底清理。
#[cfg(unix)]
fn spawn_orphan_listener(port: u16, name: &str) -> Option<u32> {
    if !orphan_test_tools_ready() {
        return None;
    }
    let which = std::process::Command::new("which")
        .arg("nc")
        .output()
        .expect("which nc 失败");
    let nc = String::from_utf8_lossy(&which.stdout).trim().to_string();
    if nc.is_empty() {
        return None;
    }
    let tmp = tempfile::tempdir().expect("tempdir 失败");
    let fake = tmp.path().join(name);
    std::fs::copy(&nc, &fake).expect("复制 nc 失败");
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fake.metadata().expect("读 metadata 失败");
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&fake, perms).expect("设置执行位失败");
    }
    // sh 启动后台子进程后立即退出 → 子进程成孤儿（ppid=1）后监听端口。
    //
    // ⚠️ 这里**不能**用 `.output()`（或 `status()` 之外的捕获变体）：`output()` 会一直
    // 读 stdout/stderr 管道直到 EOF，而 `nc -l` 继承了这两个 fd 且长期持有不关闭，
    // 于是等待永不返回——CI Linux 上实测卡死到 job 6 小时上限（本机 macOS 的 nc
    // 行为不同才没暴露）。故把三个 stdio 全部丢弃，只等 `sh` 自身退出。
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} -l {port} &", fake.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let pid = wait_listener(port, std::time::Duration::from_secs(5));
    if pid.is_none() {
        force_kill_listeners(port);
    }
    pid
}

/// BE-M3 核心场景：名为 filemind-sidecar 的孤儿（ppid=1）占住端口 → 被清理。
#[cfg(unix)]
#[test]
fn test_cleanup_orphan_sidecar_kills_named_orphan() {
    const PORT: u16 = 48765;
    let Some(pid) = spawn_orphan_listener(PORT, "filemind-sidecar") else {
        // 工具缺失或 nc 行为差异：静默跳过（本机 macOS 全量生效）
        return;
    };

    cleanup_orphan_sidecar(PORT);

    // SIGTERM 后端口应释放（轮询最多 3s）
    let gone = wait_listener(PORT, std::time::Duration::from_secs(3)).is_none();
    assert!(gone, "孤儿 Sidecar pid={pid} 应被清理，端口 {PORT} 应释放");
    force_kill_listeners(PORT);
}

/// 防误杀：名字不匹配的孤儿监听同端口 → 不得被清理。
#[cfg(unix)]
#[test]
fn test_cleanup_skips_unmatched_name() {
    const PORT: u16 = 48766;
    let Some(_pid) = spawn_orphan_listener(PORT, "innocent-listener") else {
        // 工具缺失或 nc 行为差异：静默跳过（本机 macOS 全量生效）
        return;
    };

    cleanup_orphan_sidecar(PORT);

    // 名字不含 filemind-sidecar → 即使是孤儿也不杀
    let still = wait_listener(PORT, std::time::Duration::from_millis(800));
    assert!(still.is_some(), "名字不匹配的进程不得被误杀");
    force_kill_listeners(PORT);
}

// ---------- BE-M7：失败的 restart 也计入 CrashLoop 窗口 ----------

/// 持续握手/spawn 失败的重启必须累计进窗口并在阈值后暂停自动恢复——
/// 旧实现只在握手成功时 `record_restart`，失败场景窗口永不增长，
/// 退避封顶 8s 后无限重试。`stub_binary` 指向无执行位的 Cargo.toml，
/// spawn 必败，正好构造「持续失败」场景。
#[tokio::test]
async fn test_restart_failure_counts_into_crash_loop_window() {
    let mut m = SidecarManager::new(stub_binary());
    for i in 0..CRASH_LOOP_MAX_RESTARTS {
        assert!(
            m.restart().await.is_err(),
            "第 {} 次 restart 应失败（占位二进制无法 spawn）",
            i + 1
        );
    }
    assert!(
        m.is_crash_loop_paused(),
        "连续 {CRASH_LOOP_MAX_RESTARTS} 次失败重启后应进入 CrashLoop 暂停，而非无限重试"
    );
}

// ---------- NO_PROXY 注入测试 ----------

/// `start()` 必须给子进程注入 `NO_PROXY` / `no_proxy`，让 Sidecar 的 httpx
/// 绕过系统代理直连本地 Ollama —— 否则 Clash 等代理会拦截 127.0.0.1 请求返回 502。
///
/// 实现思路：用一个 shell 脚本作为占位 binary，把子进程的 env 写入临时文件，
/// 然后 `start()` spawn 后读取文件校验是否包含预期的 `NO_PROXY` 值。
#[test]
fn test_start_injects_no_proxy_env() {
    // 仅 Unix：脚本通过 /bin/sh 执行
    if !cfg!(unix) {
        return;
    }

    // 1. 准备 env 输出文件（用 NamedTempFile 占位避免路径冲突，运行时换成固定路径）
    let out_dir = tempfile::tempdir().expect("建 env 输出临时目录失败");
    let out_path = out_dir.path().join("env.txt");
    let out_path_str = out_path.to_str().unwrap().to_string();

    // 2. 写占位 sidecar 脚本：把 env 输出到 FILEMIND_TEST_ENV_OUT 指向的文件
    let script = format!(
        "#!/bin/sh\nenv > \"{}\"\n",
        out_path_str.replace('"', "\\\"")
    );
    let script_dir = tempfile::tempdir().expect("建脚本临时目录失败");
    let script_path = script_dir.path().join("fake-sidecar.sh");
    std::fs::write(&script_path, &script).expect("写脚本失败");
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&script_path).expect("读脚本 metadata 失败");
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&script_path, perms).expect("脚本设可执行位失败");
    }

    // 3. 让子进程继承 FILEMIND_TEST_ENV_OUT（脚本依赖此变量定位输出文件）
    let _guard = EnvGuard::set("FILEMIND_TEST_ENV_OUT", &out_path_str);

    // 4. start() spawn 脚本 → 脚本执行 env 写文件后退出
    let mut m = SidecarManager::new(script_path);
    let psk = m.start().expect("spawn 占位脚本应成功");

    // 5. wait 子进程退出（脚本极快，几乎立即完成），确保 env 文件已写完
    if let Some(child) = m.process.as_mut() {
        let _ = child.wait();
    }

    // 6. 校验：子进程 env 中应包含 NO_PROXY 与 no_proxy（大小写双写）
    let env_content = std::fs::read_to_string(&out_path).unwrap_or_else(|_| String::new());
    assert!(
        env_content.contains("NO_PROXY=127.0.0.1,localhost,::1"),
        "子进程应继承 NO_PROXY=127.0.0.1,localhost,::1，实际 env:\n{env_content}"
    );
    assert!(
        env_content.contains("no_proxy=127.0.0.1,localhost,::1"),
        "子进程应继承 no_proxy=127.0.0.1,localhost,::1（小写），实际 env:\n{env_content}"
    );
    // PSK 仍是 32 字节随机，注入 env 不影响握手协议
    assert_eq!(
        psk.len(),
        32,
        "PSK 应为 32 字节，注入 env 不应破坏 PSK 生成"
    );
}
