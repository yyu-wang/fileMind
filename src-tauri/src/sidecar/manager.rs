//! Sidecar 进程生命周期管理：启动、握手、健康探测、崩溃重启与优雅停止。
//!
//! 启动流程：
//! 1. 生成 PSK（32 字节随机）→ 通过 stdin 注入 Sidecar
//! 2. 轮询 /health 直到 Sidecar 就绪
//! 3. POST /handshake 完成身份验证
//! 4. 返回 PSK，由调用方存入 `AppState` 供后续请求签名
//!
//! 运行期：
//! - 由外部（`main.rs`）`tokio::spawn` 调 `health_watchdog_step` 循环探活
//! - 连续 3 次 /health 失败或 `Child::try_wait` 返回已退出 → `restart()`
//! - 重启采用指数退避 1s→2s→4s→8s 上限；1 分钟内 10 次 → `SidecarCrashLoop` 暂停自动恢复
//!
//! 停止：
//! - `stop_graceful()` 先请求 POST /shutdown，等 3s 看 Sidecar 自退
//! - 仍存活则 `kill()` + `wait()` 兜底；总耗时 ≤ 5s
//! - `stopped` 标志位 + 内部 ``OnceCell`` 保证 stop 只真实执行一次（`Drop` 兜底不再重复杀）

use std::collections::VecDeque;
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::security::handshake;
use crate::sidecar::proxy;
use std::process::Child;

const SIDECAR_PORT: u16 = 8765;
/// Sidecar 就绪轮询最大尝试次数。
const MAX_READY_ATTEMPTS: u32 = 50;
/// 每次就绪轮询间隔（毫秒）。
const READY_POLL_INTERVAL_MS: u64 = 100;
/// 连续 /health 失败阈值：达到后认为 Sidecar 挂了，触发重启。
/// watchdog 外部 tick=1s × 3 次 = 最多 3s 检测出应用层挂死。
const HEALTH_FAIL_THRESHOLD: u32 = 3;
/// 指数退避初始值（毫秒）：首次重启失败后下一次等待 1s。
const RESTART_BACKOFF_BASE_MS: u64 = 1000;
/// 指数退避上限（毫秒）：无论失败多少次，最多等 8s 再试。
const RESTART_BACKOFF_CAP_MS: u64 = 8000;
/// `CrashLoop` 观察窗口（秒）：此窗口内重启次数超过阈值则暂停自动恢复。
/// 直接 `u32`：对应 `AppError::SidecarCrashLoop.window_secs` 字段类型，
/// 使用处 `Duration::from_secs` 接受 `u64`，再转一次不丢。
const CRASH_LOOP_WINDOW_SECS: u32 = 60;
/// `CrashLoop` 阈值：窗口内最大允许的重启次数。
const CRASH_LOOP_MAX_RESTARTS: u32 = 10;
/// 优雅关闭：`POST /shutdown` 后等待 Sidecar 自退的最大时长（秒）。
const GRACEFUL_SELF_EXIT_SECS: u64 = 3;
/// 优雅关闭：`kill` 之后 `wait` 兜底 + 余量总超时（秒），总 `DoD` ≤ 5s。
const GRACEFUL_TOTAL_TIMEOUT_SECS: u64 = 5;

/// Sidecar 进程管理器：持有子进程句柄，析构时自动停止。
pub struct SidecarManager {
    /// 子进程句柄（未启动时为 `None`）。
    process: Option<Child>,
    /// Sidecar 监听端口。
    port: u16,
    /// 当前 Sidecar 握手后的 PSK（`restart` 后替换为新 PSK）。
    psk: Option<Vec<u8>>,
    /// 最近重启时间戳队列：用于 `CrashLoop` 窗口阈值统计。
    recent_restarts: VecDeque<Instant>,
    /// 重启失败连续次数：控制指数退避；成功握手后清零。
    consecutive_failures: u32,
    /// 最近一次 /health 失败累计次数；成功即清零。
    recent_health_fails: u32,
    /// 是否已执行过真实停止动作：`stop_graceful` 首次 `set`，
    /// `Drop` 中检查为 `true` 则跳过，避免 `on_exit` 主路径 + `Drop` 重复 `kill`。
    stopped: AtomicBool,
}

impl SidecarManager {
    /// 创建管理器（默认端口，尚未启动进程）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            process: None,
            port: SIDECAR_PORT,
            psk: None,
            recent_restarts: VecDeque::new(),
            consecutive_failures: 0,
            recent_health_fails: 0,
            stopped: AtomicBool::new(false),
        }
    }

    /// 是否已停止（一次性置位）。
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// 当前 PSK：握手成功后为 `Some`，重启后会换成新 PSK。
    #[must_use]
    pub fn psk(&self) -> Option<&[u8]> {
        self.psk.as_deref()
    }

    /// 当前子进程 PID（未启动/已退出后 Zombie 等待被 wait 前仍返回旧 pid）。
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(Child::id)
    }

    /// 启动 Sidecar 子进程，通过 stdin 注入 PSK，返回 PSK 给调用方。
    ///
    /// # Errors
    ///
    /// 无法定位二进制、PSK 生成、进程拉起或 stdin 写入失败时返回 `SidecarUnavailable`。
    pub fn start(&mut self) -> AppResult<Vec<u8>> {
        // 安全：PSK 每次 start 调用新生成，避免跨会话密钥复用
        let psk = handshake::generate_psk()?;
        let psk_hex = hex::encode(&psk);

        let binary_path = std::env::current_dir()
            .map_err(|e| AppError::SidecarUnavailable(format!("无法获取当前目录: {e}")))?
            .join("binaries/filemind-sidecar");

        let mut child = std::process::Command::new(&binary_path)
            .env("SIDECAR_PORT", self.port.to_string())
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 启动失败: {e}")))?;

        // 通过 stdin 注入 PSK（hex 字符串 + 换行），随后关闭管道
        // 安全：stdin 管道仅在父子进程间可见，比 env 更稳妥（防同用户进程 ps 读取）
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(psk_hex.as_bytes())
                .map_err(|e| AppError::SidecarUnavailable(format!("stdin 写入 PSK 失败: {e}")))?;
            stdin
                .write_all(b"\n")
                .map_err(|e| AppError::SidecarUnavailable(format!("stdin 写入换行失败: {e}")))?;
            // stdin 在此处 drop，关闭管道让 Python 端 readline 返回
        }

        self.process = Some(child);
        self.psk = Some(psk.clone());
        // 新进程启动：连续失败计数重置（但若仍在 crash loop window 内，队列保持以
        // 判定暂停自动恢复）
        self.recent_health_fails = 0;
        Ok(psk)
    }

    /// 轮询 /health 直到 Sidecar 就绪或超时。
    ///
    /// # Errors
    ///
    /// 超过最大尝试次数仍未就绪时返回 `SidecarUnavailable`。
    pub async fn wait_ready(&self) -> AppResult<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        for attempt in 0..MAX_READY_ATTEMPTS {
            let ready = reqwest::get(&url)
                .await
                .is_ok_and(|resp| resp.status().is_success());
            if ready {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(READY_POLL_INTERVAL_MS)).await;
            log::debug!(
                "等待 Sidecar 就绪，尝试 {}/{}",
                attempt + 1,
                MAX_READY_ATTEMPTS
            );
        }
        Err(AppError::SidecarUnavailable(
            "Sidecar 启动超时，未在规定时间内就绪".into(),
        ))
    }

    /// 执行握手协议：生成 nonce + POST /handshake + 验证 proof。
    ///
    /// # Errors
    ///
    /// 见 [`handshake::perform_handshake`] 的错误说明。
    pub async fn handshake(&self, psk: &[u8]) -> AppResult<()> {
        let nonce = handshake::generate_nonce()?;
        let base_url = format!("http://127.0.0.1:{}", self.port);
        handshake::perform_handshake(&base_url, psk, &nonce).await
    }

    /// 启动 Sidecar 并完成握手：返回新 PSK（供 `AppState` 同步）。
    ///
    /// 成功时内部会：
    /// - 清零连续失败计数
    /// - 记录一次重启到窗口队列（便于后续 `CrashLoop` 判定，首次启动也计入
    ///   不影响，因为 `CRASH_LOOP_MAX_RESTARTS` 足够大）
    ///
    /// # Errors
    ///
    /// 任何启动或握手步骤失败时返回对应错误，**不会**清除进程：
    /// 调用方应决定是否再次尝试重试（`restart`/指数退避）。
    pub async fn start_with_handshake(&mut self) -> AppResult<Vec<u8>> {
        let psk = self.start()?;
        self.wait_ready().await?;
        self.handshake(&psk).await?;
        // 握手成功：记一次 restart 窗口事件 + 清零相关计数
        self.record_restart();
        self.consecutive_failures = 0;
        log::info!("Sidecar 握手成功");
        Ok(psk)
    }

    /// 尝试非阻塞 wait：若子进程已退出则立即返回 `Some(ExitStatus)`。
    /// 若未启动或未退出，返回 `None`（不会阻塞）。
    pub fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.process.as_mut()?.try_wait().ok().flatten()
    }

    /// 探测 Sidecar 健康状态（请求 `/health`）。
    ///
    /// # Errors
    ///
    /// 请求本身异常（而非 HTTP 状态码）时返回错误。
    pub async fn health_check(&self) -> AppResult<bool> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        Ok(reqwest::get(&url)
            .await
            .is_ok_and(|resp| resp.status().is_success()))
    }

    /// 看门狗单次步进（由外层 tokio 循环调用）。
    ///
    /// 返回值：
    /// - `Ok(WatchdogAction::Idle)`：一切正常，外层应 `sleep(1s)` 再调
    /// - `Ok(WatchdogAction::NeedRestart)`：判定 Sidecar 挂了，外层
    ///   应调用 `next_backoff()` 等待后调 `restart()`；成功后把新 PSK
    ///   回写 `AppState.sidecar_psk` 并 reset `request_seq`
    /// - `Err(SidecarCrashLoop)`：窗口内重启过多，暂停自动恢复
    ///
    /// 本方法不阻塞 `sleep`（退避等待交给调用方决定），避免内部持有 Mutex
    /// 的 `MutexGuard` 阻塞时间过长。
    ///
    /// # Errors
    ///
    /// 除上述 `SidecarCrashLoop` 外，若健康检查的 HTTP 请求遇到底层网络
    /// 异常（DNS/连接失败等）也会透出；当前实现视为"健康失败"计入连续
    /// 失败计数，但外部捕获该错误可用于排障日志。
    pub async fn watchdog_tick(&mut self) -> AppResult<WatchdogAction> {
        if self.is_stopped() {
            return Ok(WatchdogAction::Idle);
        }
        if self.is_crash_loop_paused() {
            return Err(AppError::SidecarCrashLoop {
                count: CRASH_LOOP_MAX_RESTARTS,
                window_secs: CRASH_LOOP_WINDOW_SECS,
                message: "请检查 Sidecar 日志或系统资源，手动修复后重启应用".into(),
            });
        }

        // 1) 先看 Child 是否已经退出（能最快检测 kill 导致的崩溃，不必等 /health）
        if self.try_wait().is_some() {
            log::warn!("Sidecar 进程已退出，触发重启");
            self.recent_health_fails = 0;
            return Ok(WatchdogAction::NeedRestart);
        }

        // 2) 再调 /health，累计失败次数达到阈值判为挂
        let healthy = match self.health_check().await {
            Ok(h) => h,
            Err(e) => {
                log::warn!("Sidecar health 请求异常: {e}");
                false
            }
        };
        if healthy {
            self.recent_health_fails = 0;
            return Ok(WatchdogAction::Idle);
        }
        self.recent_health_fails = self.recent_health_fails.saturating_add(1);
        if self.recent_health_fails >= HEALTH_FAIL_THRESHOLD {
            log::warn!(
                "Sidecar 健康检查连续失败 {} 次，触发重启",
                self.recent_health_fails
            );
            self.recent_health_fails = 0;
            return Ok(WatchdogAction::NeedRestart);
        }
        Ok(WatchdogAction::Idle)
    }

    /// 下次重启前应等待的退避时长（指数：1s→2s→4s→8s 封顶）。
    #[must_use]
    pub fn next_backoff(&self) -> Duration {
        let base = RESTART_BACKOFF_BASE_MS;
        let shift = u32::min(self.consecutive_failures, 3); // 2^3=8 即 cap
        let ms = u64::saturating_mul(base, 1u64 << shift);
        Duration::from_millis(u64::min(ms, RESTART_BACKOFF_CAP_MS))
    }

    /// 重启 Sidecar：`stop()`（硬杀，不走 shutdown 因为已崩溃）→
    /// `start_with_handshake()`；返回新 PSK。
    ///
    /// 重启失败会递增 `consecutive_failures` 以推动下一次更长退避。
    ///
    /// # Errors
    ///
    /// 与 [`Self::start_with_handshake`] 相同：stop 或 启动/握手任一步失败。
    pub async fn restart(&mut self) -> AppResult<Vec<u8>> {
        let _ = self.stop_hard();
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let psk = self.start_with_handshake().await?;
        Ok(psk)
    }

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
    /// # Errors
    ///
    /// `kill` 发送信号失败（非「进程不存在」）或 `wait` 系统调用失败时返回错误。
    pub fn stop_hard(&mut self) -> AppResult<()> {
        if let Some(ref mut child) = self.process {
            // kill 本身可能返回 "进程已不存在"，这对我们是 OK 的
            let kill_result = child.kill();
            match kill_result {
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
            child
                .wait()
                .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 等待失败: {e}")))?;
        }
        self.process = None;
        Ok(())
    }

    // ---------- Crash Loop 窗口 ----------

    /// 记录一次成功的重启/启动到窗口队列，并丢弃队首超出窗口的老条目。
    fn record_restart(&mut self) {
        let now = Instant::now();
        let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
        // 从队首清理超出窗口的旧记录
        while self
            .recent_restarts
            .front()
            .is_some_and(|&t| now.duration_since(t) > window)
        {
            let _ = self.recent_restarts.pop_front();
        }
        self.recent_restarts.push_back(now);
    }

    /// 是否已进入 `CrashLoop` 暂停状态：窗口内重启数超过阈值即 `true`。
    #[must_use]
    pub fn is_crash_loop_paused(&mut self) -> bool {
        // 先把老条目清掉（调用方 tick 前先清），再看长度是否超阈值
        let now = Instant::now();
        let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
        while self
            .recent_restarts
            .front()
            .is_some_and(|&t| now.duration_since(t) > window)
        {
            let _ = self.recent_restarts.pop_front();
        }
        // len 在 usize 判断，避免 as u32 转换 lint
        usize::try_from(CRASH_LOOP_MAX_RESTARTS)
            .is_ok_and(|threshold| self.recent_restarts.len() >= threshold)
    }

    /// 窗口内重启计数（用于测试与状态面板）。
    #[must_use]
    pub fn restart_count_in_window(&mut self) -> u32 {
        // 清超窗 + 计数
        let now = Instant::now();
        let window = Duration::from_secs(u64::from(CRASH_LOOP_WINDOW_SECS));
        while self
            .recent_restarts
            .front()
            .is_some_and(|&t| now.duration_since(t) > window)
        {
            let _ = self.recent_restarts.pop_front();
        }
        // 窗口内元素上限 CRASH_LOOP_MAX_RESTARTS（10）< u32::MAX，所以最多 u32::MAX；
        // 用 saturating_into 语义（超过就封顶），不丢正确性。
        u32::try_from(self.recent_restarts.len()).unwrap_or(u32::MAX)
    }
}

/// Watchdog 一次步进的返回值：由外层 tokio 循环解释动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    /// 正常：无需操作，外层 sleep 默认轮询间隔后再 tick。
    Idle,
    /// 需要重启：外层 sleep `next_backoff()` 后调用 `restart()`。
    NeedRestart,
}

impl Default for SidecarManager {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
// 测试场景下 `expect()` 表达「这条路径必须成功，否则测试直接挂掉」是合理语义。
#[allow(clippy::expect_used)]
#[path = "manager_tests.rs"]
mod tests;
