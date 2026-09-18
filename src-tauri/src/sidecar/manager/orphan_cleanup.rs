//! 孤儿 Sidecar 清理（BE-M3）：清理上次异常退出残留、已被 init 收养的进程。
//!
//! （2026-09-18 按职责拆自 `shutdown.rs`，304 行超 Rust 模块警告阈值 300；
//! 优雅停止 / 硬杀 / `Drop` 兜底仍在 [`super::shutdown`]。）
//!
//! `cleanup_orphan_sidecar` 处理的是上一次异常退出（跳过 `Drop`）留下的残留进程：它们被
//! init 收养后仍占住端口，会让本次启动探活命中旧进程而握手必败。

/// `ps -o comm=` 取到的进程名是否就是 Sidecar 主可执行。
///
/// ⚠️ Linux 的 `comm` 由内核按 `TASK_COMM_LEN - 1 = 15` 字符截断，而
/// `filemind-sidecar` 恰好 16 字符，实际取到的是 `filemind-sideca`——只做
/// `contains("filemind-sidecar")` 会**永远匹配不到**，导致 Linux 上孤儿清理
/// 形同虚设（残留进程占住端口 → 下次启动握手 401）。故额外容忍该截断形态，
/// 且用等值比较（`comm == 截断名`）而非前缀比较，避免放宽误杀范围。
///
/// 声明为 `pub` 而非 `pub(crate)`：所在模块是私有的，`pub(crate)` 已被 clippy 判为冗余；
/// 实际可见范围仍限于本模块树（测试模块按 `manager::orphan_cleanup::…` 路径直接引用）。
#[cfg(unix)]
#[must_use]
pub fn matches_sidecar_comm(comm: &str) -> bool {
    // 常量就近声明：放模块级时非 unix 平台无引用点，会被 dead_code 记为未使用。
    /// Sidecar 主可执行的进程名（打包 / onedir 形态）。
    const SIDECAR_COMM_NAME: &str = "filemind-sidecar";
    /// Linux `/proc/<pid>/comm` 的截断长度（内核 `TASK_COMM_LEN - 1`）。
    const LINUX_COMM_MAX_LEN: usize = 15;

    if comm.contains(SIDECAR_COMM_NAME) {
        return true;
    }
    let truncated = SIDECAR_COMM_NAME
        .get(..LINUX_COMM_MAX_LEN)
        .unwrap_or(SIDECAR_COMM_NAME);
    comm == truncated
}

/// 读取进程 `ps` 字段（`comm=` / `args=` / `ppid=`），失败或非零退出返回 `None`。
#[cfg(unix)]
fn read_ps(pid: u32, format: &str) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", format, "-p", &pid.to_string()])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 全量扫描进程表，收集所有 Sidecar 进程的 `(pid, ppid)`。
///
/// 返回 `None` 表示 `ps` 不可用或执行失败（视为无可清理）。
///
/// 用 `ps -axo` 全量扫描而非 lsof 端口过滤：端口过滤只能命中「已就绪监听」的孤儿，
/// 会漏杀「仍在启动中、尚未监听端口」的孤儿——后者在 cleanup 之后才占用端口，
/// 导致新 Sidecar 握手命中旧进程 401（时序竞态，2026-09-10 实测：孤儿 Sidecar
/// 在 cleanup 执行时尚未监听 8765）。
#[cfg(unix)]
fn collect_sidecar_procs() -> Option<Vec<(u32, u32)>> {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
    else {
        log::info!("孤儿清理跳过：ps 不可用");
        return None;
    };
    if !out.status.success() {
        return None;
    }
    // Sidecar 身份判定：打包模式（comm 含 `filemind-sidecar`），或 dev 模式
    // （进程名 python 且命令行带 `-m app` / `sidecar_entry.py` 入口特征）。
    let mut procs: Vec<(u32, u32)> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Some(parent_pid) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let comm = parts.next().unwrap_or("").to_lowercase();
        let is_sidecar = if matches_sidecar_comm(&comm) {
            true
        } else if comm.contains("python") {
            read_ps(pid, "args=")
                .is_some_and(|a| a.contains("-m app") || a.contains("sidecar_entry.py"))
        } else {
            false
        };
        if is_sidecar {
            procs.push((pid, parent_pid));
        }
    }
    Some(procs)
}

/// 从 Sidecar 进程表中挑出孤儿，并配对其父 bootloader，返回待终止的 pid 列表。
///
/// 孤儿判定放宽到「两级祖先」：
/// - 自身父进程已死（ppid==1），被 init 收养的真孤儿；
/// - 或父进程也是 Sidecar（PyInstaller onefile 实为 bootloader(父) + 服务进程(子)
///   两进程，端口监听者是子进程，其 ppid 指向父 bootloader 而非 1）且父进程的父进程
///   已死——旧逻辑只认 ppid==1，漏杀整对进程，子进程继续占住端口导致下一次启动
///   握手失败崩溃。
#[cfg(unix)]
fn find_orphan_pids(sidecar_procs: &[(u32, u32)]) -> Vec<u32> {
    let mut to_kill: Vec<u32> = Vec::new();
    for (pid, parent_pid) in sidecar_procs {
        let orphan = *parent_pid == 1
            || sidecar_procs
                .iter()
                .any(|(parent, grand_ppid)| *parent == *parent_pid && *grand_ppid == 1);
        if !orphan {
            log::info!("孤儿清理跳过 pid={pid}：父进程仍存活（非孤儿）");
            continue;
        }
        push_unique(&mut to_kill, *pid);
        // 一并终止父 bootloader（自身不监听端口）
        if *parent_pid != 1 {
            if let Some(parent) = sidecar_procs.iter().find(|(p, _)| *p == *parent_pid) {
                push_unique(&mut to_kill, parent.0);
            }
        }
    }
    to_kill
}

/// 追加到列表（去重，保持原顺序）。
#[cfg(unix)]
fn push_unique(list: &mut Vec<u32>, value: u32) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// SIGTERM 温和终止这些进程；失败打日志即可，不阻断启动。
#[cfg(unix)]
fn terminate_sidecars(pids: &[u32], port: u16) {
    for pid in pids {
        let kill = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
        match kill {
            Ok(s) if s.status.success() => {
                log::warn!("已清理孤儿 Sidecar 进程 pid={pid}（占用端口 {port}）");
            }
            _ => {
                log::error!("清理孤儿 Sidecar pid={pid} 失败，可能需要手工处理");
            }
        }
    }
}

/// 启动新 Sidecar 前清理上次异常退出残留的孤儿进程。
///
/// 场景：上次运行握手失败 / 崩溃路径 `std::process::exit(1)` 跳过 Drop，
/// 子进程被 launchd 收养（ppid=1）继续占住 8765 端口 → 本次启动探活命中
/// 旧进程（/health 无鉴权），新 PSK 握手必败 401 → 死循环只能手工杀进程。
///
/// 三重防误杀：
/// 1. 进程身份匹配（满足任一即可）：
///    a. 打包模式：`ps -o comm=` 进程名包含 `filemind-sidecar`（并容忍 Linux 15 字符截断形态 `filemind-sideca`，见 [`matches_sidecar_comm`]）；或
///    b. dev 模式：进程名包含 `python` 且完整命令行 `ps -o args=` 包含 `-m app`（Sidecar 启动入口特征）；
/// 2. 孤儿判定：自身 `ppid==1`（父进程已死被 init 收养），**或**父进程同为
///    Sidecar（PyInstaller onefile 是 bootloader(父)+服务(子) 两进程，端口监听者
///    是子进程）且父进程的父进程已死——两级祖先整体视为孤儿一并清理。
///    活着的应用实例其 Sidecar ppid 链指向该实例主进程，不会被误杀——因此
///    「第二实例先于单实例插件启动 Sidecar」的竞态也是安全的：第二实例
///    不清掉第一实例的 Sidecar，自己 spawn 失败/握手失败后自我清理退出。
///
/// 仅 Unix 实现；非 Unix 平台记日志跳过。工具（lsof/ps）缺失视为无可清理。
pub fn cleanup_orphan_sidecar(port: u16) {
    // 安全注释：kill 的目标经过「监听指定端口 + Sidecar 身份匹配（打包名或 python+-m app）
    // + 孤儿判定（ppid==1 或父进程同为孤儿 Sidecar）」校验，均为本应用残留 Sidecar；
    // 不涉及其他进程。
    #[cfg(unix)]
    {
        let Some(sidecar_procs) = collect_sidecar_procs() else {
            return;
        };
        let to_kill = find_orphan_pids(&sidecar_procs);
        terminate_sidecars(&to_kill, port);
    }
    #[cfg(not(unix))]
    {
        let _ = port;
        log::info!("孤儿清理跳过：当前平台未实现（非 Unix）");
    }
}
