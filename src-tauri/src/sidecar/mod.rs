//! Python Sidecar 管理：进程生命周期与 HTTP 转发。

pub mod manager;
pub mod proxy;
pub mod sse;

pub use manager::{
    cleanup_orphan_sidecar, current_target_triple, resolve_bundle_binary_path,
    resolve_bundle_from_resources, resolve_dev_binary_path, CloudSidecarEnv, SidecarManager,
    WatchdogAction, SIDECAR_PORT,
};
