//! Rust 端业务服务层：纯计算 + I/O，不依赖 Tauri IPC 类型。

pub mod classifier;
/// 分类判定引擎：规则 / 启发式 / 待确认（`classifier` 的实现模块，无对外契约）。
mod classifier_engine;
/// 分类目标路径拼接与冲突标记（`classifier` 的实现模块，无对外契约）。
mod classifier_paths;
pub mod conflict_resolver;
pub mod hash_service;
pub mod log_chain;
pub mod operation_executor;
pub mod trash;
pub mod undo_executor;
pub mod undo_window;

// 测试模块独立文件（hash_service_tests.rs / conflict_resolver_tests.rs /
// operation_executor_tests.rs / undo_window_tests.rs），避免单一源文件膨胀。
#[cfg(test)]
#[path = "hash_service_tests.rs"]
mod hash_service_tests;

#[cfg(test)]
#[path = "classifier_tests.rs"]
mod classifier_tests;

#[cfg(test)]
#[path = "conflict_resolver_tests.rs"]
mod conflict_resolver_tests;

#[cfg(test)]
#[path = "operation_executor_tests.rs"]
mod operation_executor_tests;

#[cfg(test)]
#[path = "undo_window_tests.rs"]
mod undo_window_tests;
