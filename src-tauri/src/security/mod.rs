//! 安全模块：路径校验、密钥管理与推理模式切换安全阀。

pub mod keychain;
pub mod mode_switch;
pub mod path_guard;

pub use keychain::{delete_key, get_key, store_key};
pub use path_guard::{validate, validate_within_root, validate_write_target};
