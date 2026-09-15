//! 平台相关的 Sidecar 启动适配：Windows 隐藏控制台窗口 + 子进程输出落盘。
//!
//! 为何单独成文件（2026-09-15）：`manager.rs` 已超 `rules/complexity.md` 的强制阈值并
//! 登记在 `scripts/file-size-baseline.txt`（超阈值文件只允许拆分、不允许增长），
//! 故新增的平台适配逻辑放这里，`manager.rs` 只留一行调用。
//!
//! Windows 背景：Tauri 主进程是 GUI（无控制台），直接 spawn 一个 `console=True` 的
//! Sidecar 会让系统新开一个控制台窗口（用户可见的黑框，2026-09-15 实机反馈），用
//! `CREATE_NO_WINDOW` 抑制。窗口消失后，PyInstaller bootloader 的 `[PYI-xxxx:ERROR]`
//! 与 Python traceback 也就失去可见落点，故同时把 stdout/stderr 追加写入
//! `<数据目录>/logs/sidecar.log`，保证启动失败仍可离线取证。
//!
//! 全部条目 `#[cfg(windows)]`：非 Windows 平台本模块为空（macOS/Linux 保持继承
//! stdio，`tauri dev` 时可在终端直接看到 Sidecar 输出）。

#[cfg(windows)]
use std::process::Command;

/// 数据目录（与 `main.rs::get_db_path` 同一约定）：`FILEMIND_DATA_HOME` 优先，
/// 未设置时回退 `~/.filemind`；两者都拿不到返回 `None`。
#[cfg(windows)]
fn data_home() -> Option<std::path::PathBuf> {
    std::env::var("FILEMIND_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".filemind")))
}

/// Sidecar 控制台输出日志路径：`<数据目录>/logs/sidecar.log`。
#[cfg(windows)]
fn sidecar_log_path() -> Option<std::path::PathBuf> {
    Some(data_home()?.join("logs").join("sidecar.log"))
}

/// 把 Sidecar 的 stdout/stderr 追加写入 [`sidecar_log_path`]，返回是否接线成功。
///
/// 追加而非截断：重启续写同一文件（uvicorn 为 warn 级、access_log 关闭，量极小）。
/// 任一步失败（目录不可写、句柄复制失败等）只 `warn` 并返回 `false`，不阻断启动；
/// 日志内容经 `main.rs` 的 `log_redact` formatter 统一脱敏，不泄漏绝对路径。
#[cfg(windows)]
fn attach_console_log(cmd: &mut Command) -> bool {
    let Some(path) = sidecar_log_path() else {
        log::warn!("无法解析 Sidecar 日志目录（FILEMIND_DATA_HOME 与家目录均不可用），日志不落盘");
        return false;
    };
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("Sidecar 日志目录创建失败({}): {e}", dir.display());
            return false;
        }
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        log::warn!("Sidecar 日志文件打开失败({})", path.display());
        return false;
    };
    let Ok(stderr) = file.try_clone() else {
        log::warn!("Sidecar 日志文件句柄复制失败({})", path.display());
        return false;
    };
    cmd.stdout(std::process::Stdio::from(file));
    cmd.stderr(std::process::Stdio::from(stderr));
    true
}

/// 平台适配入口（仅 Windows）：`CREATE_NO_WINDOW` 抑制控制台黑框 + stdout/stderr 落盘。
///
/// 调用方（`manager.rs`）在 `#[cfg(windows)]` 下调用；macOS/Linux 不需要本模块
/// （保持继承 stdio，`tauri dev` 时可在终端直接看到 Sidecar 输出）。
#[cfg(windows)]
pub(crate) fn apply_spawn_flags(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;

    // Win32 `CREATE_NO_WINDOW`：新进程不分配控制台窗口（不影响管道读写）。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
    attach_console_log(cmd);
}
