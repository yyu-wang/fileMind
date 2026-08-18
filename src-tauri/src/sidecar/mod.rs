//! Python Sidecar 管理：进程生命周期与 HTTP 转发。

pub mod manager;
pub mod proxy;

pub use manager::{
    current_target_triple, resolve_bundle_binary_path, resolve_bundle_from_resources,
    resolve_dev_binary_path, SidecarManager, WatchdogAction,
};
