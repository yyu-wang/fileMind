use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("路径不安全: {0}")]
    UnsafePath(String),

    #[error("Sidecar 不可用: {0}")]
    SidecarUnavailable(String),

    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("推理模式切换被拒绝: 当前 {current} 模式不可自动切换")]
    ModeSwitchForbidden { current: String },

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),

    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;
