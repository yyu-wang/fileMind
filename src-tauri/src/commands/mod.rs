//! Tauri IPC 命令层：按领域拆分的命令模块。

/// 应用配置读写命令。
pub mod config;
/// 文件扫描与批量文件操作命令。
pub mod file_ops;
/// 文件内容预览命令（文本 / 图片 / PDF）。
pub mod file_preview;
/// 文件查询、搜索与统计命令。
pub mod file_query;
/// 推理模式管理与转发命令。
pub mod inference;
