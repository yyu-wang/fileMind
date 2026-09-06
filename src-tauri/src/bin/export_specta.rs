//! IPC 类型导出工具：将 Rust 命令签名导出为 TypeScript 类型定义。
//!
//! 用法：
//! ```sh
//! cargo run --bin export-specta -- --output ../src/types/ipc.ts
//! ```
//! 未传 `--output` 时回退默认 `../src/types/ipc.ts`（保持向后兼容）。

use filemind_lib::commands;

/// 默认输出路径（相对 `src-tauri/` 工作目录）。
const DEFAULT_OUTPUT: &str = "../src/types/ipc.ts";

fn main() {
    env_logger::init();

    let output_path = parse_output_arg().unwrap_or_else(|| DEFAULT_OUTPUT.to_string());

    let builder =
        tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
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
            commands::file_ops::scan_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::delete_files,
            commands::file_ops::undo_batch,
            // T6.4 文件预览（文本 / 图片 / PDF / Office 文档文本）
            commands::file_preview::read_file_preview,
            commands::file_preview::read_document_preview,
            commands::file_query::list_files,
            // T6.4 全量文件列表（虚拟滚动）
            commands::file_query::list_all_files,
            commands::file_query::search_files,
            commands::file_query::search_by_filename,
            commands::file_query::get_file_stats,
            commands::file_query::update_file_category,
            // T3.6 操作历史查询
            commands::file_query::get_operation_history,
            commands::file_query::get_batch_detail,
            commands::inference::get_inference_mode,
            commands::inference::set_inference_mode,
            commands::ollama::ollama_status,
            commands::ollama::install_embedding_model,
            commands::config::get_config,
            commands::config::update_config,
            commands::config::sign_cloud_consent,
            commands::config::revoke_cloud_consent,
            // T9.5 E2E 测试专用命令（注册于 main.rs 仅 debug；此处恒导出供前端类型引用）
            commands::e2e::e2e_get_test_dir,
            // T7.3 云端 API Key 存取（只存 Keychain、不回传完整 Key）
            commands::api_key::get_api_key_status,
            commands::api_key::set_api_key,
            commands::api_key::delete_api_key,
            // P-07 自定义云提供商管理（CRUD + base_url 查询）
            commands::cloud_providers::list_cloud_providers,
            commands::cloud_providers::upsert_cloud_provider,
            commands::cloud_providers::delete_cloud_provider,
        ]);

    if let Err(e) = builder.export(specta_typescript::Typescript::default(), &output_path) {
        log::error!("Failed to export IPC types to {output_path}: {e}");
        std::process::exit(1);
    }

    log::info!("IPC types exported to {output_path}");
}

/// 解析命令行参数 `--output <path>`，未提供时返回 `None`。
///
/// 仅识别 `--output <path>` 与 `--output=<path>` 两种形式；其他参数忽略。
fn parse_output_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--output=") {
            return Some(value.to_string());
        }
        if arg == "--output" {
            return args.next();
        }
    }
    None
}
