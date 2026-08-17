//! Sidecar 进程生命周期管理：启动、停止与存活探测。

use crate::error::{AppError, AppResult};
use std::process::Child;

const SIDECAR_PORT: u16 = 8765;

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

    /// 启动 Sidecar 子进程。
    ///
    /// # Errors
    ///
    /// 无法定位二进制或进程拉起失败时返回 `SidecarUnavailable`。
    pub fn start(&mut self) -> AppResult<()> {
        let binary_path = std::env::current_dir()
            .map_err(|e| AppError::SidecarUnavailable(format!("无法获取当前目录: {e}")))?
            .join("binaries/filemind-sidecar");

        let child = std::process::Command::new(&binary_path)
            .env("SIDECAR_PORT", self.port.to_string())
            .spawn()
            .map_err(|e| AppError::SidecarUnavailable(format!("Sidecar 启动失败: {e}")))?;

        self.process = Some(child);
        Ok(())
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
            .map_or_else(|_| false, |resp| resp.status().is_success()))
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
