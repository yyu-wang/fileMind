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
use crate::security::{handshake, log_redact};
use crate::sidecar::proxy;
use std::process::Child;
use tauri::Manager as _;

/// Sidecar 固定监听端口（本机回环）。
pub const SIDECAR_PORT: u16 = 8765;
/// Sidecar 就绪轮询最大尝试次数。
/// 打包态 Sidecar 现包含 lancedb / numpy / pyarrow 等重依赖，冷启动实测约 39s，
/// 放宽到 60s 窗口（600 × 100ms），避免重依赖场景下握手超时。
const MAX_READY_ATTEMPTS: u32 = 600;
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

/// 云端模式需要注入 Sidecar 进程的 env（T7.4 代理接线）。
///
/// 由 Rust 在启动 Sidecar 前设置；Sidecar 侧 E8 Provider 据此调用
/// `FILEMIND_CLOUD_PROXY_URL` 代理并携带 `FILEMIND_CLOUD_PROXY_TOKEN` 鉴权头，
/// `FILEMIND_CLOUD_MASKING` 触发 T7.2 云端脱敏。
pub struct CloudSidecarEnv {
    /// Rust 云端代理地址（`http://127.0.0.1:{CLOUD_PROXY_PORT}`）。
    pub proxy_url: String,
    /// 代理调用方共享 token（Sidecar 请求时放 `X-FileMind-Token`）。
    pub proxy_token: String,
    /// 是否启用云端数据脱敏（T7.2，`inference_mode=cloud` 时为真）。
    pub masking_on: bool,
}

/// Sidecar 进程管理器：持有子进程句柄，析构时自动停止。
pub struct SidecarManager {
    /// Sidecar 二进制绝对路径，由调用方在构造时显式注入。
    ///
    /// dev 模式：`resolve_dev_binary_path()` 解析自 env / repo 相对路径；
    /// bundle 模式：Tauri v2 `app.path().resolve(...)` 解析自 `MacOS` / 安装目录。
    ///
    /// 字段名加下划线后缀避免与同名访问器方法 [`SidecarManager::binary_path`] 冲突
    /// （字段私有，仅内部实现访问；对外一律通过访问器）。
    binary_path_: std::path::PathBuf,
    /// 子进程句柄（未启动时为 `None`）。
    process: Option<Child>,
    /// Sidecar 监听端口。
    port: u16,
    /// 云端模式 env 注入（`None` = 本地模式，不注入；重启后自动保持）。
    cloud_env: Option<CloudSidecarEnv>,
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
    /// 用调用方解析好的 Sidecar 二进制绝对路径创建管理器（尚未启动进程）。
    ///
    /// 启动语义：本函数不校验 `binary_path` 是否存在，若路径无效，会在
    /// [`SidecarManager::start`] 的 `Command::spawn` 阶段返回
    /// [`AppError::SidecarUnavailable`]（附路径信息，便于排障）。
    #[must_use]
    pub const fn new(binary_path: std::path::PathBuf) -> Self {
        Self {
            binary_path_: binary_path,
            process: None,
            port: SIDECAR_PORT,
            cloud_env: None,
            psk: None,
            recent_restarts: VecDeque::new(),
            consecutive_failures: 0,
            recent_health_fails: 0,
            stopped: AtomicBool::new(false),
        }
    }

    /// 当前 Sidecar 二进制路径（供排障面板 / 日志展示）。
    #[must_use]
    pub fn binary_path(&self) -> &std::path::Path {
        &self.binary_path_
    }

    /// 测试场景：拿到构造时内部 `binary_path` 引用（同 crate 可见，避免对外公开字段）。
    #[cfg(test)]
    pub(crate) fn binary_path_inner(&self) -> &std::path::Path {
        &self.binary_path_
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

    /// 设置云端模式 env 注入（须在 `start` 之前调用；重启自动沿用）。
    pub fn set_cloud_env(&mut self, cloud_env: CloudSidecarEnv) {
        self.cloud_env = Some(cloud_env);
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

        log::info!(
            "准备启动 Sidecar，binary={}",
            log_redact::sanitize_path(&self.binary_path_.display().to_string())
        );

        let mut cmd = std::process::Command::new(&self.binary_path_);
        cmd.env("SIDECAR_PORT", self.port.to_string());
        // T7.4：云端模式注入代理地址/token/脱敏开关（重启后经字段保持）
        if let Some(cloud) = &self.cloud_env {
            cmd.env("FILEMIND_CLOUD_PROXY_URL", &cloud.proxy_url);
            cmd.env("FILEMIND_CLOUD_PROXY_TOKEN", &cloud.proxy_token);
            cmd.env(
                "FILEMIND_CLOUD_MASKING",
                if cloud.masking_on { "1" } else { "0" },
            );
        }
        // 本地服务（Ollama / Sidecar 自身）必须绕过系统代理（Clash 等），
        // 否则 httpx 读 http_proxy 走代理 → 本地 127.0.0.1 被代理拦截返回 502。
        // httpx 同时检查 NO_PROXY 与 no_proxy，双写最稳妥（不同系统读法不一）。
        cmd.env("NO_PROXY", "127.0.0.1,localhost,::1");
        cmd.env("no_proxy", "127.0.0.1,localhost,::1");
        cmd.stdin(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            AppError::SidecarUnavailable(format!(
                "Sidecar 启动失败 (binary={}): {e}",
                self.binary_path_.display()
            ))
        })?;

        // 通过 stdin 注入 PSK（hex 字符串 + 换行），随后关闭管道
        // 安全：stdin 管道仅在父子进程间可见，比 env 更稳妥（防同用户进程 ps 读取）
        // BE-M3：任一步写失败都必须先 kill 再返回 Err——局部 child 直接 drop
        // 不会杀进程（std Child 无 kill_on_drop），否则泄漏的孤儿进程占住
        // 8765 端口，后续启动探活命中旧进程导致握手死循环。
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(psk_hex.as_bytes()) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::SidecarUnavailable(format!(
                    "stdin 写入 PSK 失败: {e}"
                )));
            }
            if let Err(e) = stdin.write_all(b"\n") {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::SidecarUnavailable(format!(
                    "stdin 写入换行失败: {e}"
                )));
            }
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
            // 用 3s 超时的探活 client：Sidecar「接受连接但不响应」的挂死态
            // 下，无超时的请求会无限挂起，导致整个启动流程卡死（BE-C4）
            let ready = proxy::probe_client()
                .get(&url)
                .send()
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
    /// 失败时（BE-M6）：统一走 [`Self::abort_failed_start`]——杀掉刚拉起的
    /// 进程并清除未验证的 PSK。不允许出现「进程活着但未完成握手」的中间
    /// 态：否则 watchdog /health 探活成功判 Idle 永不重启，而 `AppState`
    /// 里没有匹配 PSK → 所有代理请求永久 401。
    ///
    /// # Errors
    ///
    /// 任何启动或握手步骤失败时返回对应错误（进程已被清理）。
    pub async fn start_with_handshake(&mut self) -> AppResult<Vec<u8>> {
        let psk = self.start()?;
        // 就绪轮询 / 握手任一失败：杀进程 + 清未验证 PSK，让下一轮重启走完整流程
        if let Err(e) = self.wait_ready().await {
            self.abort_failed_start();
            return Err(e);
        }
        if let Err(e) = self.handshake(&psk).await {
            self.abort_failed_start();
            return Err(e);
        }
        // 握手成功：记一次 restart 窗口事件 + 清零相关计数
        self.record_restart();
        self.consecutive_failures = 0;
        log::info!("Sidecar 握手成功");
        Ok(psk)
    }

    /// 启动/握手失败的统一清理：杀掉未完成握手的子进程并清除 PSK。
    ///
    /// 不置 `stopped` 标志：那是「应用主动优雅关闭」语义，置位会让 watchdog
    /// 判定已停止而永久退出，重启退避循环失效。`stop_hard` 已把 `process`
    /// 置 None，Drop 兜底重跑是无害 no-op。
    fn abort_failed_start(&mut self) {
        log::warn!("Sidecar 启动/握手失败，清理未验证的子进程");
        let _ = self.stop_hard();
        self.psk = None;
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
        // Default 仅用于 Mutex::new(Default::default()) 类型占位或单测；
        // 真实二进制运行前（main/setup）会被具体解析后的路径覆盖。
        // 拼接 ``${CARGO_MANIFEST_DIR}/../filemind/binaries/filemind-sidecar``，
        // 即便文件不存在，也保证 binary_path() 字段语义对应约定的 dev 产物位置，
        // 不会再误指向 Cargo.toml 文本。
        let dev_stub = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("filemind")
            .join("binaries")
            .join("filemind-sidecar");
        Self::new(dev_stub)
    }
}

// ---------- 二进制路径解析（dev 模式 / CI 注入） ----------

/// 根据当前编译目标推断 Rust triple 字符串（用于定位 `filemind-sidecar-{triple}` 产物名）。
///
/// 注意：triple 字符串匹配的是 **构建侧** `build-sidecar.sh --target` 的参数。
/// 对于「本机编译本机跑」场景一致；交叉编译环境下由 `FILEMIND_SIDECAR_BINARY` 覆盖，
/// 不会走到该回退。
#[must_use]
pub fn current_target_triple() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin".into(),
        ("macos", "x86_64") => "x86_64-apple-darwin".into(),
        ("windows", "x86_64") => "x86_64-pc-windows-msvc".into(),
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu".into(),
        (os, arch) => format!("{arch}-{os}"),
    }
}

/// dev 模式解析 Sidecar 二进制绝对路径（打包模式由 Tauri `PathResolver` 代替本函数）。
///
/// 优先级从高到低：
///
/// 1. `override_env`：调用方读 `FILEMIND_SIDECAR_BINARY` 后传入（绝对/相对都行，
///    存在即 canonicalize 返回）；传 `None` 或空串 → 跳过。
/// 2. 基于 `CARGO_MANIFEST_DIR` 环境变量（cargo 注入，指向 ``<repo>/src-tauri``）：
///    向上回退到 repo 根，然后找 ``filemind/binaries/``：
///    a. ``filemind-sidecar-{triple}`` 具体架构产物（优先生效）
///    b. ``filemind-sidecar`` 软链接（兜底，build-sidecar.sh 创建）
/// 3. 最后回退：``${cwd}/filemind/binaries/filemind-sidecar``（兼容手工启动场景）。
///
/// 返回：第一个命中且 `metadata().is_file()` 的路径（已 `canonicalize`，无相对段）。
///
/// # Errors
///
/// 全部候选路径不存在时返回 [`AppError::SidecarUnavailable`]，错误信息附带候选列表
/// + 当前 triple 构建建议，便于排障。
pub fn resolve_dev_binary_path(override_env: Option<&str>) -> AppResult<std::path::PathBuf> {
    let mut tried: Vec<String> = Vec::new();

    // ---- 优先级 1：显式覆盖（CI / 调试） ----
    if let Some(ov) = override_env.filter(|s| !s.is_empty()) {
        let p = std::path::PathBuf::from(ov);
        tried.push(format!("(override) {}", p.display()));
        if is_existing_file(&p) {
            return p.canonicalize().map_err(|e| {
                AppError::SidecarUnavailable(format!("canonicalize override 失败: {e}"))
            });
        }
    }

    // ---- 优先级 2：CARGO_MANIFEST_DIR → repo 根回退 ----
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let src_tauri = std::path::PathBuf::from(manifest_dir);
        // CARGO_MANIFEST_DIR = <repo>/src-tauri → repo 根 = parent()
        if let Some(repo_root) = src_tauri.parent() {
            let binaries_dir = repo_root.join("filemind").join("binaries");
            let triple = current_target_triple();
            let cand_triple = binaries_dir.join(format!("filemind-sidecar-{triple}"));
            tried.push(format!("(cargo-triple) {}", cand_triple.display()));
            if is_existing_file(&cand_triple) {
                return cand_triple.canonicalize().map_err(|e| {
                    AppError::SidecarUnavailable(format!("canonicalize triple-binary 失败: {e}"))
                });
            }
            let cand_sym = binaries_dir.join("filemind-sidecar");
            tried.push(format!("(cargo-symlink) {}", cand_sym.display()));
            if is_existing_file(&cand_sym) {
                return cand_sym.canonicalize().map_err(|e| {
                    AppError::SidecarUnavailable(format!("canonicalize sidecar-symlink 失败: {e}"))
                });
            }
        }
    }

    // ---- 优先级 3：cwd 兜底 ----
    if let Ok(cwd) = std::env::current_dir() {
        let fallback = cwd
            .join("filemind")
            .join("binaries")
            .join("filemind-sidecar");
        tried.push(format!("(cwd) {}", fallback.display()));
        if is_existing_file(&fallback) {
            return fallback.canonicalize().map_err(|e| {
                AppError::SidecarUnavailable(format!("canonicalize cwd fallback 失败: {e}"))
            });
        }
    } else {
        tried.push("(cwd) 无法读取 current_dir → 已跳过".to_string());
    }

    // ---- 全部不命中 ----
    Err(AppError::SidecarUnavailable(format!(
        "dev 模式未找到 Sidecar 二进制，候选列表:\n  - {}\n\
         建议：1) 先跑 bash scripts/build-sidecar.sh --target {}；2) 或设置 FILEMIND_SIDECAR_BINARY 指向产物绝对路径",
        tried.join("\n  - "),
        current_target_triple()
    )))
}

/// 小 helper：`fs::metadata(p).ok()?.is_file()` 走短路，不用再写多处。
fn is_existing_file(p: &std::path::Path) -> bool {
    std::fs::metadata(p).ok().is_some_and(|m| m.is_file())
}

// ---------- 二进制路径解析（bundle 模式 / Tauri resources 回退） ----------

/// bundle 模式下，基于给定的「resources 根目录」解析 Sidecar 可执行文件的绝对路径。
///
/// 纯函数：`resolve_bundle_binary_path`（Tauri 封装版）对 `AppHandle` 的 `PathResolver`
/// 结果再调用本函数；单测可绕过 Tauri 直接传 `tempdir` 验证拼接和错误文案。
///
/// 候选查找顺序：
/// 1. `${resources_root}/filemind-sidecar-{triple}`（架构专属，优先生效）
/// 2. `${resources_root}/filemind-sidecar`（Windows 上额外兼容 `.exe` 后缀兜底）
///
/// # Errors
///
/// 全部候选不存在 / 非文件 → 返回 [`AppError::SidecarUnavailable`]，附候选路径列表
/// 与 `resources_root`，便于现场排障（如打包脚本漏拷了二进制）。
pub fn resolve_bundle_from_resources(
    resources_root: &std::path::Path,
) -> AppResult<std::path::PathBuf> {
    let triple = current_target_triple();
    let mut tried: Vec<String> = Vec::new();

    let candidates: Vec<std::path::PathBuf> = if cfg!(windows) {
        vec![
            resources_root.join(format!("filemind-sidecar-{triple}.exe")),
            resources_root.join("filemind-sidecar.exe"),
            resources_root.join(format!("filemind-sidecar-{triple}")),
            resources_root.join("filemind-sidecar"),
        ]
    } else {
        vec![
            resources_root.join(format!("filemind-sidecar-{triple}")),
            resources_root.join("filemind-sidecar"),
        ]
    };

    for c in candidates {
        tried.push(format!("{}", c.display()));
        if is_existing_file(&c) {
            return c.canonicalize().map_err(|e| {
                AppError::SidecarUnavailable(format!(
                    "Sidecar 命中 bundle 候选 {} 但 canonicalize 失败: {e}",
                    c.display()
                ))
            });
        }
    }

    Err(AppError::SidecarUnavailable(format!(
        "Sidecar 二进制在 Tauri resources 目录下未找到: resources_root={}; 候选列表:\n  {}\n请确认打包脚本 build-sidecar.sh 已把产物拷入 resources/",
        resources_root.display(),
        tried.join("\n  ")
    )))
}

/// bundle 模式下，通过 Tauri [`tauri::Manager::path`] 解析 Sidecar 可执行文件绝对路径。
///
/// 解析到的路径即传给 [`SidecarManager::new`] 启动；本函数仅做路径定位，不含进程启动。
///
/// 实现层：先取 `app.path().resource_dir()` → 命中再调纯函数
/// [`resolve_bundle_from_resources`]。这样单测可以不用 Mock Tauri Runtime。
///
/// # Errors
///
/// - `app.path().resource_dir()` 返回 `None`（极少：非 bundle 环境或平台不支持）
/// - resources 下所有候选均不命中（详情见 [`resolve_bundle_from_resources`]）
pub fn resolve_bundle_binary_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<std::path::PathBuf> {
    let root = app.path().resource_dir().map_err(|e| {
        AppError::SidecarUnavailable(format!(
            "Tauri resource_dir 查询失败（非 bundle 环境？）: {e}"
        ))
    })?;
    resolve_bundle_from_resources(&root)
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

/// 启动新 Sidecar 前清理上次异常退出残留的孤儿进程。
///
/// 场景：上次运行握手失败 / 崩溃路径 `std::process::exit(1)` 跳过 Drop，
/// 子进程被 launchd 收养（ppid=1）继续占住 8765 端口 → 本次启动探活命中
/// 旧进程（/health 无鉴权），新 PSK 握手必败 401 → 死循环只能手工杀进程。
///
/// 三重防误杀：
/// 1. 端口匹配：仅处理监听 `port`（默认 8765）的进程（`lsof -ti tcp:{port}` 前置过滤）；
/// 2. 进程身份匹配（满足任一即可）：
///    a. 打包模式：`ps -o comm=` 进程名包含 `filemind-sidecar`；或
///    b. dev 模式：进程名包含 `python` 且完整命令行 `ps -o args=` 包含 `-m app`（Sidecar 启动入口特征）；
/// 3. `ps -o ppid=` 必须为 1（真孤儿，父进程已死被 init 收养）。
///    活着的应用实例其 Sidecar ppid 是该实例主进程，不会被误杀——因此
///    「第二实例先于单实例插件启动 Sidecar」的竞态也是安全的：第二实例
///    不清掉第一实例的 Sidecar，自己 spawn 失败/握手失败后自我清理退出。
///
/// 仅 Unix 实现；非 Unix 平台记日志跳过。工具（lsof/ps）缺失视为无可清理。
pub fn cleanup_orphan_sidecar(port: u16) {
    // 安全注释：kill 的目标经过「监听指定端口 + Sidecar 身份匹配（打包名或 python+-m app）
    // + ppid==1」三重校验，均为本应用残留 Sidecar；不涉及其他进程。
    #[cfg(unix)]
    {
        let lsof = std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{port}")])
            .output();
        let Ok(out) = lsof else {
            log::info!("孤儿清理跳过：lsof 不可用");
            return;
        };
        if !out.status.success() {
            return; // 无进程监听该端口（lsof 非零退出）——正常情况
        }
        let pids = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse::<u32>().ok())
            .collect::<Vec<u32>>();
        for pid in pids {
            // 校验 1：进程身份匹配（打包模式 OR dev 模式）
            let comm_output = std::process::Command::new("ps")
                .args(["-o", "comm=", "-p", &pid.to_string()])
                .output();
            let comm_str = comm_output
                .ok()
                .filter(|c| c.status.success())
                .map(|c| String::from_utf8_lossy(&c.stdout).to_lowercase());
            let bundle_matches = comm_str
                .as_deref()
                .is_some_and(|s| s.contains("filemind-sidecar"));
            // dev 模式：进程名是 python，且完整命令行带 `-m app` 入口
            let dev_matches = if comm_str.as_deref().is_some_and(|s| s.contains("python")) {
                let args_output = std::process::Command::new("ps")
                    .args(["-o", "args=", "-p", &pid.to_string()])
                    .output();
                args_output.is_ok_and(|a| {
                    a.status.success() && String::from_utf8_lossy(&a.stdout).contains("-m app")
                })
            } else {
                false
            };
            if !bundle_matches && !dev_matches {
                log::info!("孤儿清理跳过 pid={pid}：进程名/命令行不匹配");
                continue;
            }
            // 校验 2：ppid == 1（父进程已死，被 init 收养的真孤儿）
            let ppid = std::process::Command::new("ps")
                .args(["-o", "ppid=", "-p", &pid.to_string()])
                .output();
            let is_orphan = ppid.is_ok_and(|c| {
                c.status.success() && String::from_utf8_lossy(&c.stdout).trim() == "1"
            });
            if !is_orphan {
                log::info!("孤儿清理跳过 pid={pid}：父进程仍存活（非孤儿）");
                continue;
            }
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

#[cfg(test)]
// 测试场景下 `expect()` 表达「这条路径必须成功，否则测试直接挂掉」是合理语义。
#[allow(clippy::expect_used)]
#[path = "manager_tests.rs"]
mod tests;
