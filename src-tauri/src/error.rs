//! 应用统一错误类型与结果别名。

use thiserror::Error;

/// 应用级错误，涵盖路径安全、Sidecar、数据库、IO、网络与序列化。
#[derive(Debug, Error)]
pub enum AppError {
    /// 路径未通过安全校验（路径遍历或黑名单命中）。
    #[error("路径不安全: {0}")]
    UnsafePath(String),

    /// Python Sidecar 进程不可用或通信失败。
    #[error("Sidecar 不可用: {0}")]
    SidecarUnavailable(String),

    /// `SQLite` 数据库操作错误。
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    /// 推理模式切换被安全阀拒绝。
    #[error("推理模式切换被拒绝: 当前 {current} 模式不可自动切换")]
    ModeSwitchForbidden {
        /// 被拒绝时所处的当前模式。
        current: String,
    },

    /// 文件系统 IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// 网络请求错误。
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),

    /// JSON 序列化/反序列化错误。
    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

/// 统一结果类型别名。
pub type AppResult<T> = Result<T, AppError>;
