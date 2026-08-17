use crate::error::{AppError, AppResult};
use std::process::Child;

const SIDECAR_PORT: u16 = 8765;
const HEALTH_CHECK_INTERVAL_MS: u64 = 2000;
const MAX_RESTART_ATTEMPTS: u32 = 3;

pub struct SidecarManager {
    process: Option<Child>,
    port: u16,
}

impl SidecarManager {
    pub fn new() -> Self {
        Self {
            process: None,
            port: SIDECAR_PORT,
        }
    }

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

    pub async fn health_check(&self) -> AppResult<bool> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        match reqwest::get(&url).await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
