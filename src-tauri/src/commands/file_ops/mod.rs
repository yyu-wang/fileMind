//! 文件操作命令：扫描、预览、批量执行与撤销。
//!
//! 所有命令做阻塞文件系统操作，通过 `#[tauri::command(async)]`
//! 声明为线程池执行，避免阻塞主线程。
//!
//! 模块划分（原单文件 1395 行按职责拆分，各文件 < 300 行，见 `rules/complexity.md`）：
//!   - `scan`        扫描命令 + 增量落库 + `FileInfo` → `FileRecord` 转换
//!   - `fs_walk`     磁盘遍历底层（递归、系统/隐藏目录与文件黑名单、时间格式化）
//!   - `preview`     预览（dry-run）计划与汇总
//!   - `execute`     批量执行
//!   - `delete`      删除到系统回收站
//!   - `undo`        批次撤销
//!   - `directories` 扫描目录管理
//!   - `index_sync`  向量索引 best-effort 同步
//!
//! 路径变化：命令注册需用子模块全路径（如 `file_ops::scan::scan_directory`，原因见下方
//! 再导出处的说明），`main.rs` 与 `bin/export_specta` 已同步；类型与工具函数
//! （`OperationType` / `PlanItem` / `scan_and_persist` 等）仍在 `file_ops` 根上再导出，
//! 故 `services::operation_executor` 与 `tests/scan_perf` 无需改动。

pub mod delete;
pub mod directories;
pub mod execute;
mod fs_walk;
mod index_sync;
pub mod preview;
pub mod scan;
pub mod undo;

// —— 对外再导出 ——
// 注意：**命令无法靠再导出给宏用**。`#[tauri::command]` / `#[specta::specta]` 生成的
// 隐藏辅助项（`__cmd__*` / `__specta__fn__*`）只存在于命令定义所在模块，宏是按「给定
// 路径的父模块」去找它们的，`pub use` 不会把辅助项带过来。因此 `main.rs` /
// `bin/export_specta.rs` 注册的是子模块全路径（如 `file_ops::scan::scan_directory`）；
// 下面仅再导出类型与工具函数，供非宏调用方沿用拆分前的路径：
pub use delete::DeleteFilesResult;
pub use directories::RemoveDirectoryResponse;
pub use execute::{ExecuteRequest, ExecuteResponse, ExecuteResult, ExecuteSummary};
pub use preview::{
    BatchResult, FileOperation, OperationPreview, OperationResult, OperationType, PlanItem,
    PreviewRequest, PreviewResponse, PreviewSummary,
};
pub use scan::scan_and_persist;
pub use undo::UndoResponse;

// —— 测试模块 ——
// 5 个测试文件原先是「同文件内联测试」的组织：用 `use super::*` 直接调内部实现。
// 拆分后内部项归属子模块（子模块里声明为 `pub(super)`），故测试模块改为**显式引入**
// （见各 *_tests.rs 顶部），不再依赖本模块额外转发，避免出现「只为测试存在的 import 清单」。
// 与生产模块同目录，`super` 即 `file_ops`；每个模块各自 #[cfg(test)]，不得只在首个上
// 标注——否则其余模块会进非测试构建，并因引用 cfg(test) 模块而编译失败。
#[cfg(test)]
mod execute_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod scan_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod undo_tests;
