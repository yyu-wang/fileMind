//! 启动与握手：把 Sidecar 进程拉起来并完成身份验证。
//!
//! 流程：`start`（spawn + PSK 经 stdin/env 注入）→ `wait_ready`（轮询 `/health`）→
//! `handshake`（nonce + proof 校验）。`start_with_handshake` 串起三步，任一步失败都走
//! `abort_failed_start` 清理，避免留下「进程活着但未完成握手」的中间态——那会让
//! watchdog 探活成功判 Idle 而 `AppState` 里没有匹配 PSK，所有代理请求永久 401。

use super::{
    CloudSidecarEnv, LocalLlmSidecarEnv, SidecarManager, StartupTimings, MAX_READY_ATTEMPTS,
    READY_POLL_INTERVAL_MS,
};
use crate::error::{AppError, AppResult};
use crate::security::{handshake, log_redact};
use crate::sidecar::proxy;
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// 注入 Sidecar 需要的「自定义配置」env（云端代理 T7.4 + 本地生成后端 T3b）。
///
/// 独立成函数的原因：`start` 已触及 clippy 的 `too_many_lines` 阈值（60），
/// 且这两段注入与启动流程无关，只是把 Rust 侧已知的配置翻译成 env。
fn apply_injected_env(
    cmd: &mut std::process::Command,
    cloud: Option<&CloudSidecarEnv>,
    local: Option<&LocalLlmSidecarEnv>,
) {
    // 云端模式注入代理地址/token/脱敏开关
    if let Some(cloud) = cloud {
        cmd.env("FILEMIND_CLOUD_PROXY_URL", &cloud.proxy_url);
        cmd.env("FILEMIND_CLOUD_PROXY_TOKEN", &cloud.proxy_token);
        cmd.env(
            "FILEMIND_CLOUD_MASKING",
            if cloud.masking_on { "1" } else { "0" },
        );
        // P-07：注入激活提供商 slug（Sidecar ProviderFactory 据此选
        // GenericCloudProvider，空串不注入，Python 端 env 读不到即回落内置规则）
        if !cloud.active_cloud_provider.is_empty() {
            cmd.env(
                "FILEMIND_ACTIVE_CLOUD_PROVIDER",
                &cloud.active_cloud_provider,
            );
        }
    }
    // T3b：本地生成后端配置。builtin 时 Sidecar 会拉起随包分发的 llama.cpp 引擎
    // 作为子进程；未安装 Ollama 的部署机器依赖它完成知识问答。
    if let Some(local) = local {
        cmd.env("FILEMIND_LOCAL_LLM_BACKEND", &local.backend);
        cmd.env("FILEMIND_LOCAL_LLM_MODEL", &local.model);
    }
}

impl SidecarManager {
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
        // PSK 双通路注入（优先 env，stdin 兜底）：dev 模式 bash wrapper 脚本可能
        // 在启动过程中意外消费 stdin 首行（shell profile / heredoc 处理等），
        // 导致 Python 端 readline 读到空串或脏数据，握手签名校验失败返回 401。
        // 通过 env 通路保证无论 shell 层行为如何，PSK_HEX 都能精确到达 Python
        // 端 _inject_psk() 的 env 分支（优先级高于 stdin）。
        // 安全：env PSK 同用户其他进程可读，但 dev 模式仅本机，且与打包态 stdin
        // 主路径语义独立，不降低生产（PyInstaller 二进制 + stdin only）安全。
        cmd.env("PSK_HEX", &psk_hex);
        // 数据目录：传递 FILEMIND_DATA_HOME 给 Python Sidecar，保证 SQLite 与 LanceDB 落在同一根
        // 未设置时不传递，Python 端会使用默认的 ~/.filemind
        if let Ok(data_home) = std::env::var("FILEMIND_DATA_HOME") {
            cmd.env("FILEMIND_DATA_HOME", data_home);
        }
        // T7.4 + T3b：云端代理与本地生成后端的 env 注入
        apply_injected_env(
            &mut cmd,
            self.cloud_env.as_ref(),
            self.local_llm_env.as_ref(),
        );
        // 本地服务（Ollama / Sidecar 自身）必须绕过系统代理（Clash 等），
        // 否则 httpx 读 http_proxy 走代理 → 本地 127.0.0.1 被代理拦截返回 502。
        // httpx 同时检查 NO_PROXY 与 no_proxy，双写最稳妥（不同系统读法不一）。
        cmd.env("NO_PROXY", "127.0.0.1,localhost,::1");
        cmd.env("no_proxy", "127.0.0.1,localhost,::1");
        // Windows：抑制控制台黑框 + 子进程输出落盘（实现在 sidecar/platform.rs）
        #[cfg(windows)]
        crate::sidecar::platform::apply_spawn_flags(&mut cmd);
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
    /// 每轮先探一次子进程是否已退出：打包态启动失败（DLL 装载失败等）时进程会**秒退**，
    /// 不做这步前端要盲等满 `MAX_READY_ATTEMPTS`（60s）才看到「失败」——实测侧车倒在
    /// `PyInstaller` bootloader 报错时 0.2s 就退出（2026-09-15 实机排障）。
    ///
    /// # Errors
    ///
    /// 子进程提前退出（附退出状态，便于前端展示）或超过最大尝试次数仍未就绪时
    /// 返回 `SidecarUnavailable`。
    pub async fn wait_ready(&mut self) -> AppResult<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        for attempt in 0..MAX_READY_ATTEMPTS {
            // 子进程已退出 → 立即失败（不再空等）：错误信息带退出状态，前端「引擎启动失败」
            // 提示与日志可直接看到原因，而不是「启动超时」这种无信息量的兜底文案。
            // 注：`try_wait` 会回收子进程，`stop_hard` 已改为容忍「已退出」状态。
            if let Some(status) = self.try_wait() {
                return Err(AppError::SidecarUnavailable(format!(
                    "Sidecar 进程已退出（{status}），未能就绪"
                )));
            }
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
    /// - 记录本次分阶段耗时到 [`SidecarManager::last_startup`]（P3-1），
    ///   并按 `sidecar.startup_ms` 口径打一条 info 日志（首启与 watchdog 重启共用本路径）
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
        // P3-1：进入即清空上次记录，失败路径不再回填——下游读到的 `Some` 一定是本次成功
        self.last_startup = None;
        let started = Instant::now();

        let spawn_started = Instant::now();
        let psk = self.start()?;
        let spawn = spawn_started.elapsed();

        let ready_started = Instant::now();
        // 就绪轮询 / 握手任一失败：杀进程 + 清未验证 PSK，让下一轮重启走完整流程
        if let Err(e) = self.wait_ready().await {
            self.abort_failed_start();
            return Err(e);
        }
        let ready = ready_started.elapsed();

        let handshake_started = Instant::now();
        if let Err(e) = self.handshake(&psk).await {
            self.abort_failed_start();
            return Err(e);
        }
        let handshake_elapsed = handshake_started.elapsed();

        // 握手成功：记一次 restart 窗口事件 + 清零相关计数
        self.record_restart();
        self.consecutive_failures = 0;
        let timings = StartupTimings {
            spawn,
            ready,
            handshake: handshake_elapsed,
            total: started.elapsed(),
        };
        self.last_startup = Some(timings);
        log::info!("Sidecar 握手成功");
        log::info!(
            "Sidecar 启动耗时（sidecar.startup_ms）: total_ms={} spawn_ms={} ready_ms={} handshake_ms={}",
            timings.total.as_millis(),
            timings.spawn.as_millis(),
            timings.ready.as_millis(),
            timings.handshake.as_millis()
        );
        Ok(psk)
    }

    /// 启动/握手失败的统一清理：杀掉未完成握手的子进程并清除 PSK。
    ///
    /// 不置 `stopped` 标志：那是「应用主动优雅关闭」语义，置位会让 watchdog
    /// 判定已停止而永久退出，重启退避循环失效。`stop_hard` 已把 `process`
    /// 置 None，Drop 兜底重跑是无害 no-op。
    ///
    /// `pub(super)`：本模块外只有测试模块直接调用（同为 `manager` 的后代）。
    pub(super) fn abort_failed_start(&mut self) {
        log::warn!("Sidecar 启动/握手失败，清理未验证的子进程");
        let _ = self.stop_hard();
        self.psk = None;
    }

    /// 尝试非阻塞 wait：若子进程已退出则立即返回 `Some(ExitStatus)`。
    /// 若未启动或未退出，返回 `None`（不会阻塞）。
    pub fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.process.as_mut()?.try_wait().ok().flatten()
    }
}
