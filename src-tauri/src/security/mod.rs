//! 安全模块：路径校验、密钥管理、推理模式切换安全阀与 Sidecar 握手协议。

/// 云端 LLM 请求代理（Sidecar → Rust → 云端，Key 不经过 Python）。
pub mod cloud_proxy;
/// Sidecar 握手协议与请求签名（HMAC-SHA256 + nonce + 序号防重放）。
pub mod handshake;
/// API Key 安全存储（系统 Keychain）。
pub mod keychain;
/// 日志脱敏过滤器（I-03）：API Key / 绝对路径统一替换占位符。
pub mod log_redact;
/// 推理模式切换安全阀（用户同意校验）。
pub mod mode_switch;
/// 路径安全守卫（黑名单与越界校验）。
pub mod path_guard;

pub use cloud_proxy::generate_token;
pub use keychain::{delete_key, get_key, store_key};
pub use path_guard::{
    validate, validate_relative_subpath, validate_within_root, validate_write_target,
};
