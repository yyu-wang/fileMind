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

    /// 握手协议失败：响应签名不匹配、Sidecar 身份未通过验证。
    /// 安全映射：S-01（Sidecar 端口冒充）。
    #[error("握手失败: {0}")]
    HandshakeFailed(String),

    /// 请求签名验证失败：签名无效或序号重放。
    /// 安全映射：T-01（Sidecar 通信篡改）。
    #[error("请求签名验证失败: {0}")]
    RequestSignatureInvalid(String),

    /// Sidecar 崩溃次数超过窗口阈值：暂停自动重启以避免死循环 burn CPU。
    /// 上层应用应弹出错误面板提示用户手动介入（检查 Sidecar 日志/系统资源）。
    #[error("Sidecar 崩溃重启过于频繁 ({count} 次/{window_secs}s)，已暂停自动恢复：{message}")]
    SidecarCrashLoop {
        /// 触发阈值时的重启计数。
        count: u32,
        /// 观察窗口（秒）。
        window_secs: u32,
        /// 诊断信息（例如「请检查 Sidecar 日志」）。
        message: String,
    },

    /// `SQLite` 数据库操作错误。
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    /// 系统密钥链（Keychain / Credential Manager）读写失败。
    /// 区别于 `SidecarUnavailable`：密钥链故障与 Sidecar 进程无关，
    /// 且读失败时必须中止写入，防止聚合写回静默清空全部密钥。
    #[error("密钥链访问失败: {0}")]
    Keychain(String),

    /// 推理模式切换被安全阀拒绝。
    #[error("推理模式切换被拒绝: 当前 {current} 模式不可自动切换")]
    ModeSwitchForbidden {
        /// 被拒绝时所处的当前模式。
        current: String,
    },

    /// 业务规则禁止操作（如删除系统内置分类、修改不可变字段）。
    #[error("操作被禁止: {0}")]
    Forbidden(String),

    /// 用户输入参数校验失败（如 `move` 操作未提供 `target_dir`、`file_ids` 为空）。
    /// 用于 IPC 命令入口的轻量校验，区别于 `Forbidden` 的业务规则与 `UnsafePath` 的安全策略。
    #[error("参数无效: {0}")]
    InvalidInput(String),

    /// 文件系统 IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// 网络请求错误。
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),

    /// 内部状态错误（如 DB 锁中毒、不变量被破坏）。不向用户暴露实现细节。
    #[error("内部错误: {0}")]
    Internal(String),

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
