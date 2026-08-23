//! Tauri IPC 命令层：按领域拆分的命令模块。

/// 云端 API Key 存取命令（安全 07-§4 / T7.3，只存 Keychain、不回传完整 Key）。
pub mod api_key;
/// RAG 对话流式命令（Sidecar `/chat/stream` SSE 代理）。
pub mod chat;
/// 智能分类预览命令（规则引擎 + 启发式）。
pub mod classify;
/// 应用配置读写命令。
pub mod config;
/// 文件扫描与批量文件操作命令。
pub mod file_ops;
/// 文件内容预览命令（文本 / 图片 / PDF）。
pub mod file_preview;
/// 文件查询、搜索与统计命令。
pub mod file_query;
/// 文件索引建立命令（Sidecar `/index/build` 代理）。
pub mod index;
/// 推理模式管理与转发命令。
pub mod inference;
/// Ollama 推理环境探测命令（Sidecar `/inference/test` 代理）。
pub mod ollama;
/// 规则编辑命令（规则 CRUD + 优先级拖拽排序）。
pub mod rules;
