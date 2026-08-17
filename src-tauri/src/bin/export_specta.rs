use filemind_lib::commands;

fn main() {
    let builder = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::file_ops::scan_directory,
            commands::file_ops::preview_operations,
            commands::file_ops::execute_operations,
            commands::file_ops::undo_batch,
            commands::file_query::list_files,
            commands::file_query::search_files,
            commands::file_query::search_by_filename,
            commands::file_query::get_file_stats,
            commands::file_query::update_file_category,
            commands::inference::get_inference_mode,
            commands::inference::set_inference_mode,
            commands::config::get_config,
            commands::config::update_config,
        ]);

    if let Err(e) = builder.export(specta_typescript::Typescript::default(), "../src/types/ipc.ts") {
        eprintln!("Failed to export IPC types: {e}");
        std::process::exit(1);
    }
}
