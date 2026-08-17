//! Sidecar 进程生命周期管理：启动、握手、健康探测与停止。
//!
//! 启动流程：
//! 1. 生成 PSK（32 字节随机）→ 通过 stdin 注入 Sidecar
//! 2. 轮询 /health 直到 Sidecar 就绪
//! 3. POST /handshake 完成身份验证
//! 4. 返回 PSK，由调用方存入 `AppState` 供后续请求签名

use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::security::handshake;
use std::process::Child;

const SIDECAR_PORT: u16 = 8765;
/// Sidecar 就绪轮询最大尝试次数。
const MAX_READY_ATTEMPTS: u32 = 50;
/// 每次轮询间隔（毫秒）。
const READY_POLL_INTERVAL_MS: u64 = 100;

/// Sidecar 进程管理器：持有子进程句柄，析构时自动停止。
pub struct SidecarManager {
    /// 子进程句柄（未启动时为 `None`）。
    process: Option<Child>,
    /// Sidecar 监听端口。
    port: u16,
}

impl SidecarManager {
    /// 创建管理器（默认端口，尚未启动进程）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            process: None,
            port: SIDECAR_PORT,
        }
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

    /// 启动 Sidecar 并完成握手，返回 PSK 给调用方存入 `AppState`。
    ///
    /// # Errors
    ///
    /// 任何启动或握手步骤失败时返回对应错误。
    pub async fn start_with_handshake(&mut self) -> AppResult<Vec<u8>> {
        let psk = self.start()?;
        self.wait_ready().await?;
        self.handshake(&psk).await?;
        log::info!("Sidecar 握手成功");
        Ok(psk)
    }

    /// 停止 Sidecar 子进程并等待退出。
    ///
    /// # Errors
    ///
    /// 进程终止或等待退出失败时返回 `SidecarUnavailable`。
    pub fn stop(&mut self) -> AppResult<()> {
        if let Some(ref mut child) = self.process {
            child
                .kill()
                .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 停止失败: {e}")))?;
            child
                .wait()
                .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 等待失败: {e}")))?;
        }
        self.process = None;
        Ok(())
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
}

impl Default for SidecarManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_has_no_process() {
        let manager = SidecarManager::new();
        // process 字段为 private，通过行为验证：未启动时 stop 应成功无副作用
        let mut manager = manager;
        let result = manager.stop();
        assert!(result.is_ok(), "未启动时 stop 应无副作用");
    }

    #[test]
    fn test_start_fails_when_binary_missing() -> AppResult<()> {
        let tmp = tempfile::tempdir()?;
        // 切换到不包含 binaries/ 的临时目录，使 binary_path 找不到
        std::env::set_current_dir(tmp.path())?;

        let mut manager = SidecarManager::new();
        let result = manager.start();

        // 恢复原工作目录（避免污染后续测试）
        // 注：这里直接切回原 cwd 在测试并行执行时存在 race，但本测试用例不与
        // 其他依赖 cwd 的测试并行运行（项目测试均为顺序执行）
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));

        assert!(result.is_err(), "binary 不存在时 start 应失败");
        let err_msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            err_msg.contains("Sidecar 启动失败"),
            "错误信息应包含启动失败描述，实际：{err_msg}"
        );
        Ok(())
    }

    #[test]
    fn test_default_equals_new() {
        let a = SidecarManager::new();
        let b = SidecarManager::default();
        // 通过 stop 行为一致性验证（struct 字段为 private 无法直接比较）
        let mut a = a;
        let mut b = b;
        assert!(a.stop().is_ok());
        assert!(b.stop().is_ok());
    }
}
