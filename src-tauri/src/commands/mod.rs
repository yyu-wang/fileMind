//! Tauri IPC 命令层：按领域拆分的命令模块。

/// 云端 API Key 存取命令（安全 07-§4 / T7.3，只存 Keychain、不回传完整 Key）。
pub mod api_key;
/// RAG 对话流式命令（Sidecar `/chat/stream` SSE 代理）。
pub mod chat;
/// 智能分类预览命令（规则引擎 + 启发式）。
pub mod classify;
/// 用户自定义云提供商管理（CRUD + base_url 查询，对应 cloud_providers 表）。
pub mod cloud_providers;
/// 应用配置读写命令。
pub mod config;
/// Office 文档预览的 Sidecar 抽取路径（`file_preview` 的实现模块，无命令）。
pub mod document_preview;
/// (T9.5) E2E 测试专用命令（函数常编译；注册在 main.rs 仅 debug，release 不可调用）。
pub mod e2e;
/// 向量表名解析（模型来自配置、版本来自 Sidecar 注册表，避免硬编码 `_v1`）。
pub mod embedding_table;
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
/// Embedding 模型下载命令（Sidecar `/models/download*` 代理）。
pub mod model;
/// Ollama 推理环境探测命令（Sidecar `/inference/test` 代理）。
pub mod ollama;
/// 规则编辑命令（规则 CRUD + 优先级拖拽排序）。
pub mod rules;
/// Sidecar 生命周期查询与手动重试命令（P1-1）。
pub mod sidecar;
