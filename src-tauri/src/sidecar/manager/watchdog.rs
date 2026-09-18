//! 健康探测与重启决策：判断 Sidecar 是否还活着，并给出外层该做什么。
//!
//! 探活顺序是先看 `Child::try_wait`（最快检测进程被 kill），再请求 `/health` 并累计
//! 失败次数；连续失败达阈值即返回 `NeedRestart`。重启带指数退避（1s→2s→4s→8s 封顶），
//! 窗口内重启过多则进入 `CrashLoop` 暂停自动恢复。停止动作本身在 `shutdown` 子模块，
//! 启动动作在 `start`，本模块只做判断与编排。

use super::{
    SidecarManager, CRASH_LOOP_MAX_RESTARTS, CRASH_LOOP_WINDOW_SECS, HEALTH_FAIL_THRESHOLD,
    RESTART_BACKOFF_BASE_MS, RESTART_BACKOFF_CAP_MS,
};
use crate::error::{AppError, AppResult};
use crate::sidecar::proxy;
use std::time::{Duration, Instant};

impl SidecarManager {
    /// 探测 Sidecar 健康状态（请求 `/health`）。
    ///
    /// # Errors
    ///
    /// 请求本身异常（而非 HTTP 状态码）时返回错误。
    pub async fn health_check(&self) -> AppResult<bool> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        // watchdog 持锁调用本方法：3s 超时把持锁窗口从「无限」收敛到有界（BE-C4）
        Ok(proxy::probe_client()
            .get(&url)
            .send()
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
        // P1-1：尚未启动（后台引导线程处理中 / 启动失败等待用户重试）——
        // 不探测、不重启，避免 watchdog 与 bootstrap 线程竞态双开进程占住端口。
        // 运行期崩溃后 `stop_hard` + 握手失败也会落到此态，此时交给用户重试。
        if self.process.is_none() && self.psk.is_none() {
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
        // 首次失败与每 10 次各记一条：能看到「从何时开始不响应」，又不至于刷屏
        // （加载模型等长任务期间 /health 无响应属预期，见 HEALTH_FAIL_THRESHOLD 注释）
        if self.recent_health_fails == 1 || self.recent_health_fails.is_multiple_of(10) {
            log::warn!(
                "Sidecar /health 无响应（第 {}/{} 次；加载模型等长任务期间属正常，暂不重启）",
                self.recent_health_fails,
                HEALTH_FAIL_THRESHOLD
            );
        }
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
    /// 失败会递增 `consecutive_failures` 推动更长退避，并计入 `CrashLoop`
    /// 窗口（BE-M7）：`record_restart` 原本只在握手成功时调用，持续握手
    /// 失败时窗口永不增长 → 退避封顶 8s 后无限重试，「1 分钟 10 次暂停」
    /// 失效。现在每次重启尝试恰好计一次——成功在 `start_with_handshake`
    /// 记、失败在此记（`abort_failed_start` 不计：首次启动失败不该混入
    /// 「重启窗口」）。
    ///
    /// # Errors
    ///
    /// 与 [`Self::start_with_handshake`] 相同：stop 或 启动/握手任一步失败。
    pub async fn restart(&mut self) -> AppResult<Vec<u8>> {
        let _ = self.stop_hard();
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        match self.start_with_handshake().await {
            Ok(psk) => Ok(psk),
            Err(e) => {
                // 失败的重启尝试同样占用 CrashLoop 窗口名额
                self.record_restart();
                Err(e)
            }
        }
    }

    // ---------- Crash Loop 窗口 ----------

    /// 记录一次成功的重启/启动到窗口队列，并丢弃队首超出窗口的老条目。
    ///
    /// `pub(super)`：`start_with_handshake`（start 子模块）成功后也会记一次。
    pub(super) fn record_restart(&mut self) {
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
