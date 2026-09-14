//! `SidecarManager` 测试的公共夹具：环境变量互斥、临时 onedir 目录、占位二进制构造。
//!
//! 独立成模块的原因：这些 helper 被 4 个测试模块共用（函数为 `pub(super)`）；
//! 测试模块以 `use super::support::*;` 引入——glob 不会因部分未使用而告警。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达（生产代码严格禁止）。

pub(super) use std::path::{Path, PathBuf};

// ---------- 工具：RAII 环境变量守卫，避免 set_var 串扰 ----------

/// 串行化「改进程级环境变量 / cwd」的用例。
///
/// libtest **默认多线程并行**跑用例，而 `CARGO_MANIFEST_DIR` 与 cwd 是进程级共享状态：
/// 不加锁时 A 用例设的值会被并行的 B 用例读到（实测表现为解析到别人临时目录、
/// assertion 里出现两个不同的 `.tmpXXXX`）。
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取环境变量用例互斥锁；忽略中毒（某个用例 panic 不该连带挂掉后续用例）。
pub(super) fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `env::set_var` 的 RAII 包装：测试结束时恢复（或跳过删除）原值。
///
/// 为什么不用 crate：项目 dev-deps 未引入 `temp_env`，用 10 行自写满足需求。
/// 使用方必须先取 [`lock_env`] 串行化（见其说明）。
pub(super) struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    pub(super) fn set(key: &'static str, value: impl Into<String>) -> Self {
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
pub(super) fn main_exe_name() -> &'static str {
    if cfg!(windows) {
        "filemind-sidecar.exe"
    } else {
        "filemind-sidecar"
    }
}

/// 写一个带执行位的占位「可执行文件」。
pub(super) fn write_executable(path: &Path) {
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
pub(super) fn write_onedir(repo_root: &Path, dir_name: &str, exe_name: &str) {
    let dir = repo_root.join("filemind").join("binaries").join(dir_name);
    std::fs::create_dir_all(&dir).expect("mkdir onedir 目录失败");
    write_executable(&dir.join(exe_name));
}

/// 建临时目录并在其中放置 `filemind/binaries/<dir_name>/` onedir 结构，返回 `TempDir`。
pub(super) fn build_onedir_dir(dir_name: &str, exe_name: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("建临时目录失败");
    write_onedir(tmp.path(), dir_name, exe_name);
    tmp
}

/// 写 ``<tmp>/src-tauri/Cargo.toml`` 占位文件，供 `CARGO_MANIFEST_DIR` 测试用。
pub(super) fn write_src_tauri(repo_root: &Path) {
    let src_tauri = repo_root.join("src-tauri");
    std::fs::create_dir_all(&src_tauri).expect("mkdir src-tauri 失败");
    std::fs::write(src_tauri.join("Cargo.toml"), b"[package]\nname = 'x'\n")
        .expect("写占位 Cargo.toml 失败");
}

// ---------- 原有 8 条测试：适配 new(PathBuf) ----------

pub(super) fn stub_binary() -> PathBuf {
    // 用一个真文件占位（不真启动，只做字段校验）。``<src-tauri>/Cargo.toml`` 永远存在。
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}
