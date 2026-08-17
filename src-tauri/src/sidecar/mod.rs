//! Python Sidecar 管理：进程生命周期与 HTTP 转发。

pub mod manager;
pub mod proxy;

pub use manager::SidecarManager;
