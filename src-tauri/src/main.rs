use std::path::PathBuf;
use std::sync::Mutex;

use filemind_lib::commands;
use filemind_lib::db::Database;
use filemind_lib::AppState;

fn get_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".filemind").join("filemind.db")
}

fn main() {
    let db_path = get_db_path();

    let database = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            db: Mutex::new(database),
        })
        .invoke_handler(tauri::generate_handler![
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

    if let Err(e) = builder.run(tauri::generate_context!()) {
        eprintln!("Error while running tauri application: {e}");
        std::process::exit(1);
    }
}
