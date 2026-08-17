//! `FileMind` 桌面应用入口：初始化数据库、启动 Sidecar 与握手、注册 IPC 命令并启动 Tauri。

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use filemind_lib::commands;
use filemind_lib::db::Database;
use filemind_lib::error::AppError;
use filemind_lib::sidecar::SidecarManager;
use filemind_lib::AppState;

fn get_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".filemind").join("filemind.db")
}

/// 启动 Sidecar 并完成 HMAC 握手，返回 PSK。
///
/// # Errors
///
/// 任何启动或握手步骤失败时返回对应错误。
fn start_sidecar_with_handshake(manager: &mut SidecarManager) -> Result<Vec<u8>, AppError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::SidecarUnavailable(format!("tokio runtime 初始化失败: {e}")))?;
    runtime.block_on(async { manager.start_with_handshake().await })
}

/// 应用入口：初始化日志与数据库后启动 Sidecar 与握手，注册 IPC 命令后启动 Tauri 事件循环。
///
/// `generate_context!` 宏在编译期生成较大的上下文结构体（框架行为），
/// 栈占用为 Tauri 已知模式，非业务代码问题，定点豁免此 nursery lint。
#[allow(clippy::large_stack_frames)]
fn main() {
    env_logger::init();
    let db_path = get_db_path();

    let database = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            log::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    // 启动 Sidecar 并完成 HMAC 握手：失败直接退出，避免在未验证身份时进入主循环
    let mut sidecar_manager = SidecarManager::new();
    let sidecar_psk = match start_sidecar_with_handshake(&mut sidecar_manager) {
        Ok(psk) => Some(psk),
        Err(e) => {
            log::error!("Sidecar handshake failed: {e}");
            std::process::exit(1);
        }
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState {
            db: Mutex::new(database),
            sidecar_psk: Mutex::new(sidecar_psk),
            request_seq: AtomicU64::new(0),
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
        log::error!("Error while running tauri application: {e}");
        std::process::exit(1);
    }

    // `sidecar_manager` 在 main 退出时 Drop → stop() → kill child，避免僵尸进程
}
