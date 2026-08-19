//! 安全模块：路径校验、密钥管理、推理模式切换安全阀与 Sidecar 握手协议。

/// Sidecar 握手协议与请求签名（HMAC-SHA256 + nonce + 序号防重放）。
pub mod handshake;
/// API Key 安全存储（系统 Keychain）。
pub mod keychain;
/// 推理模式切换安全阀（用户同意校验）。
pub mod mode_switch;
/// 路径安全守卫（黑名单与越界校验）。
pub mod path_guard;

pub use keychain::{delete_key, get_key, store_key};
pub use path_guard::{
    validate, validate_relative_subpath, validate_within_root, validate_write_target,
};
