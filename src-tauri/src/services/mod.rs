//! Rust 端业务服务层：纯计算 + I/O，不依赖 Tauri IPC 类型。

pub mod conflict_resolver;
pub mod hash_service;
pub mod operation_executor;

// 测试模块独立文件（hash_service_tests.rs / conflict_resolver_tests.rs /
// operation_executor_tests.rs），避免单一源文件膨胀。
#[cfg(test)]
#[path = "hash_service_tests.rs"]
mod hash_service_tests;

#[cfg(test)]
#[path = "conflict_resolver_tests.rs"]
mod conflict_resolver_tests;

#[cfg(test)]
#[path = "operation_executor_tests.rs"]
mod operation_executor_tests;
