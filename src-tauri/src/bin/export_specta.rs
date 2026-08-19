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
            commands::file_ops::scan_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::undo_batch,
            // T6.4 文件预览（文本 / 图片 / PDF）
            commands::file_preview::read_file_preview,
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
            commands::config::get_config,
            commands::config::update_config,
            commands::config::sign_cloud_consent,
            commands::config::revoke_cloud_consent,
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
