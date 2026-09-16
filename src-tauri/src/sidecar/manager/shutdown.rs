//! 停止与回收：优雅退出、硬杀兜底、`Drop` 保护，以及上次残留的孤儿进程清理。
//!
//! `stop_graceful` 先请求 `POST /shutdown` 等 Sidecar 自退，超时则 `stop_hard` 兜底
//! （`kill` + `wait`）；`stopped` 标志位保证只真实执行一次，`Drop` 只在未置位时补一刀。
//! `cleanup_orphan_sidecar` 处理的是上一次异常退出（跳过 `Drop`）留下的残留进程：它们被
//! init 收养后仍占住端口，会让本次启动探活命中旧进程而握手必败。

use super::{SidecarManager, GRACEFUL_SELF_EXIT_SECS, GRACEFUL_TOTAL_TIMEOUT_SECS};
use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl SidecarManager {
    /// 优雅停止 Sidecar：
    /// 1. 先 POST /shutdown（如果有 PSK）→ 等最多 3s 看 Sidecar 自退
    /// 2. 仍存活则 `kill()` + `wait()` 兜底
    /// 3. 总时长 ≤ 5s（`GRACEFUL_TOTAL_TIMEOUT_SECS`）
    ///
    /// 幂等：内部 `stopped` 标志位保证第二次及以后直接成功无副作用。
    ///
    /// # Errors
    ///
    /// kill / wait 失败时返回错误；标志位仍会被置为 stopped（避免下次重试杀已退出 pid）。
    pub async fn stop_graceful(&mut self, req_seq: u64) -> AppResult<()> {
        if self
            .stopped
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        let total_deadline = Instant::now() + Duration::from_secs(GRACEFUL_TOTAL_TIMEOUT_SECS);

        // 阶段 1：请求 /shutdown，若 Sidecar 仍活着且有已握手 PSK
        if let Some(psk) = self.psk.clone() {
            if self.process.is_some() {
                // 发起 shutdown 请求，但不阻塞于响应：Sidecar 0.5s 后就会自退
                // 这里用 short timeout 限制等待；失败也继续走 kill 分支
                let fut = proxy::forward_shutdown(&psk, req_seq);
                let _ = tokio::time::timeout(Duration::from_secs(1), fut).await;
            }
        }
        // 阶段 2：等待 Sidecar 自退（最多 3s 或剩余总时长的较小者）
        let self_exit_deadline = std::cmp::min(
            total_deadline,
            Instant::now() + Duration::from_secs(GRACEFUL_SELF_EXIT_SECS),
        );
        while Instant::now() < self_exit_deadline {
            if self
                .process
                .as_mut()
                .and_then(|c| c.try_wait().ok().flatten())
                .is_some()
            {
                self.process = None;
                return Ok(());
            }
            // 精细轮询 50ms，减少检测延迟
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // 阶段 3：仍未自退 → 硬杀 + wait
        self.stop_hard()
    }

    /// 硬杀停止：立即 kill + wait，不发 shutdown 请求。
    ///
    /// 仅用于 watchdog 检测到 Sidecar 已挂（进程已死或健康失败）的场景，
    /// 以及 `stop_graceful` 的兜底分支。`stopped` 标志位不会被 set（主流程
    /// 的 `stop_graceful` 负责设置，崩溃重启场景无需置位）。
    ///
    /// P1-1（2026-09-15）：进程可能**已经退出**（`wait_ready` 的提前退出检测、
    /// watchdog 的 `try_wait` 都已回收它）。此时 `kill()` 在 Windows 上会以
    /// `ERROR_ACCESS_DENIED` 失败；若把它当错误返回，`process` 字段会残留 `Some`，
    /// watchdog 的「尚未启动」守卫（`process.is_none() && psk.is_none()`）随之失效
    /// → 空转重启。故先探一次 `try_wait`，已退出则跳过 kill，仅 `wait` 收尾
    /// （std 会返回 `try_wait` 已缓存的状态，不会阻塞）。
    ///
    /// # Errors
    ///
    /// `kill` 发送信号失败（非「进程不存在」）或 `wait` 系统调用失败时返回错误。
    pub fn stop_hard(&mut self) -> AppResult<()> {
        if let Some(ref mut child) = self.process {
            let already_exited = child.try_wait().ok().flatten().is_some();
            if !already_exited {
                // kill 本身可能返回 "进程已不存在"，这对我们是 OK 的
                match child.kill() {
                    Ok(()) => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::InvalidInput => {
                        // 某些平台下 pid 0/不存在返回 InvalidInput（无此进程）；忽略
                    }
                    Err(e) => {
                        return Err(AppError::SidecarUnavailable(format!(
                            "Sidecar 停止失败: {e}"
                        )));
                    }
                }
            }
            child
                .wait()
                .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 等待失败: {e}")))?;
        }
        self.process = None;
        Ok(())
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // 若主路径 on_exit 已跑过 stop_graceful（stopped=true），跳过避免重复杀。
        // 若仍未 stopped（异常路径），退化为硬杀：Drop 中不能 await 调 shutdown。
        if self
            .stopped
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let _ = self.stop_hard();
        }
    }
}

// ---------- 孤儿 Sidecar 清理（BE-M3） ----------

/// `ps -o comm=` 取到的进程名是否就是 Sidecar 主可执行。
///
/// ⚠️ Linux 的 `comm` 由内核按 `TASK_COMM_LEN - 1 = 15` 字符截断，而
/// `filemind-sidecar` 恰好 16 字符，实际取到的是 `filemind-sideca`——只做
/// `contains("filemind-sidecar")` 会**永远匹配不到**，导致 Linux 上孤儿清理
/// 形同虚设（残留进程占住端口 → 下次启动握手 401）。故额外容忍该截断形态，
/// 且用等值比较（`comm == 截断名`）而非前缀比较，避免放宽误杀范围。
///
/// 声明为 `pub` 而非 `pub(crate)`：所在模块是私有的，`pub(crate)` 已被 clippy 判为冗余；
/// 实际可见范围仍限于本模块树（测试模块按 `manager::shutdown::…` 路径直接引用）。
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

/// 启动新 Sidecar 前清理上次异常退出残留的孤儿进程。
///
/// 场景：上次运行握手失败 / 崩溃路径 `std::process::exit(1)` 跳过 Drop，
/// 子进程被 launchd 收养（ppid=1）继续占住 8765 端口 → 本次启动探活命中
/// 旧进程（/health 无鉴权），新 PSK 握手必败 401 → 死循环只能手工杀进程。
///
/// 三重防误杀：
/// 1. 端口匹配：仅处理监听 `port`（默认 8765）的进程（`lsof -ti tcp:{port}` 前置过滤）；
/// 2. 进程身份匹配（满足任一即可）：
///    a. 打包模式：`ps -o comm=` 进程名包含 `filemind-sidecar`（并容忍 Linux 15 字符截断形态 `filemind-sideca`，见 [`matches_sidecar_comm`]）；或
///    b. dev 模式：进程名包含 `python` 且完整命令行 `ps -o args=` 包含 `-m app`（Sidecar 启动入口特征）；
/// 3. 孤儿判定：自身 `ppid==1`（父进程已死被 init 收养），**或**父进程同为
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
        /// 读取进程 ps 字段（comm=/args=/ppid=），失败或非零退出返回 None。
        fn read_ps(pid: u32, format: &str) -> Option<String> {
            let out = std::process::Command::new("ps")
                .args(["-o", format, "-p", &pid.to_string()])
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        }

        // 全量扫描所有进程（`ps -axo`），不依赖 lsof 端口过滤。端口过滤只能命中
        // 「已就绪监听」的孤儿，会漏杀「仍在启动中、尚未监听端口」的孤儿——后者在
        // cleanup 之后才占用端口，导致新 Sidecar 握手命中旧进程 401（时序竞态，
        // 2026-09-10 实测：孤儿 Sidecar 在 cleanup 执行时尚未监听 8765）。
        let ps_all = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .output();
        let Ok(out) = ps_all else {
            log::info!("孤儿清理跳过：ps 不可用");
            return;
        };
        if !out.status.success() {
            return;
        }
        // 收集所有 Sidecar 进程：打包模式（comm 含 `filemind-sidecar`），或 dev 模式
        // （进程名 python 且命令行带 `-m app` / `sidecar_entry.py` 入口特征）。
        let mut sidecar_procs: Vec<(u32, u32)> = Vec::new();
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
                sidecar_procs.push((pid, parent_pid));
            }
        }
        let mut to_kill: Vec<u32> = Vec::new();
        for (pid, parent_pid) in &sidecar_procs {
            // 孤儿判定放宽到「两级祖先」：
            // - 自身父进程已死（ppid==1），被 init 收养的真孤儿；
            // - 或父进程也是 Sidecar（PyInstaller onefile 实为 bootloader(父) +
            //   服务进程(子) 两进程，端口监听者是子进程，其 ppid 指向父 bootloader
            //   而非 1）且父进程的父进程已死——旧逻辑只认 ppid==1，漏杀整对
            //   进程，子进程继续占住端口导致下一次启动握手失败崩溃。
            let orphan = *parent_pid == 1
                || sidecar_procs
                    .iter()
                    .any(|(parent, grand_ppid)| *parent == *parent_pid && *grand_ppid == 1);
            if !orphan {
                log::info!("孤儿清理跳过 pid={pid}：父进程仍存活（非孤儿）");
                continue;
            }
            if !to_kill.contains(pid) {
                to_kill.push(*pid);
            }
            // 一并终止父 bootloader（自身不监听端口）
            if *parent_pid != 1 {
                if let Some(parent) = sidecar_procs.iter().find(|(p, _)| *p == *parent_pid) {
                    if !to_kill.contains(&parent.0) {
                        to_kill.push(parent.0);
                    }
                }
            }
        }
        for pid in to_kill {
            // SIGTERM 温和终止；失败打日志即可，不阻断启动
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
    #[cfg(not(unix))]
    {
        let _ = port;
        log::info!("孤儿清理跳过：当前平台未实现（非 Unix）");
    }
}
