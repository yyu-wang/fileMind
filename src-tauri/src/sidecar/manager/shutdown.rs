//! 停止与回收：优雅退出、硬杀兜底与 `Drop` 保护。
//!
//! `stop_graceful` 先请求 `POST /shutdown` 等 Sidecar 自退，超时则 `stop_hard` 兜底
//! （`kill` + `wait`）；`stopped` 标志位保证只真实执行一次，`Drop` 只在未置位时补一刀。
//!
//! 上次异常退出（跳过 `Drop`）留下的孤儿进程清理已按职责拆至
//! [`super::orphan_cleanup`]（2026-09-18，原文件 304 行超 Rust 模块警告阈值 300）。

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
