//! IPC 命令注册表：`generate_handler!` 清单的唯一来源。
//!
//! ⚠️ 每条必须写**命令定义所在的子模块全路径**（如 `commands::file_ops::scan::scan_directory`）：
//! `#[tauri::command]` 生成的 `__tauri_command_name_*` / `__cmd__*` 辅助项只存在于命令
//! 定义所在模块，宏按「给定路径的父模块」去解析它们（tauri-macros 2.6.3
//! `command/handler.rs`），`pub use` 再导出带不过去，会报 `cannot find __cmd__xxx`。
//!
//! 清单与 `bin/export_specta.rs` 的 `collect_commands!` 保持同一顺序，便于对照维护。

use filemind_lib::commands;

/// 构建 `Builder::invoke_handler` 用的命令表。
pub fn build() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // T6.6 RAG 问答（Sidecar /chat/stream SSE 代理）
        commands::chat::chat_stream,
        // T7.x 建立文件索引（Sidecar /index/build 代理）
        commands::index::build_index,
        // T6.5 智能分类预览（规则引擎 + 启发式）
        commands::classify::classify_preview,
        // T6.8 规则编辑（CRUD + 拖拽排序）
        commands::rules::list_rules,
        commands::rules::upsert_rule,
        commands::rules::delete_rule,
        commands::rules::reorder_rules,
        commands::rules::list_categories,
        commands::file_ops::scan::scan_directory,
        commands::file_ops::directories::list_scanned_directories,
        commands::file_ops::directories::remove_directory,
        commands::file_ops::preview::preview_operations,
        commands::file_ops::execute::execute_operations,
        commands::file_ops::delete::delete_files,
        commands::file_ops::undo::undo_batch,
        commands::file_preview::read_file_preview,
        commands::file_preview::read_document_preview,
        commands::file_query::list_files,
        commands::file_query::list_all_files,
        commands::file_query::search_files,
        commands::file_query::search_by_filename,
        commands::file_query::get_file_stats,
        commands::file_query::update_file_category,
        commands::file_query::get_operation_history,
        commands::file_query::get_batch_detail,
        commands::inference::get_inference_mode,
        commands::inference::set_inference_mode,
        commands::ollama::ollama_status,
        commands::model::model_download_status,
        commands::model::start_model_download,
        commands::config::get_config,
        commands::config::update_config,
        commands::config::sign_cloud_consent,
        commands::config::revoke_cloud_consent,
        // T7.3 云端 API Key 存取（只存 Keychain、不回传完整 Key）
        commands::api_key::get_api_key_status,
        commands::api_key::set_api_key,
        commands::api_key::delete_api_key,
        // P-07 自定义云提供商管理（对应 cloud_providers 表 CRUD）
        commands::cloud_providers::list_cloud_providers,
        commands::cloud_providers::upsert_cloud_provider,
        commands::cloud_providers::delete_cloud_provider,
        // P1-1 Sidecar 生命周期查询 / 手动重试
        commands::sidecar::get_sidecar_status,
        commands::sidecar::retry_sidecar_start,
        // T9.5 E2E 测试专用命令（仅 debug 注册；release 不携带自动化入口）
        #[cfg(debug_assertions)]
        commands::e2e::e2e_get_test_dir,
    ]
}
