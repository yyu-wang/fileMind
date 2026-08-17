//! 开发辅助工具：在用户目录初始化 `FileMind` 数据库并执行迁移。
//!
//! 供测试数据脚本（`scripts/gen_testdata.py`）在数据库不存在时引导初始化，
//! 也可手动执行：
//! `cargo run --example init_db --manifest-path src-tauri/Cargo.toml`

fn main() -> Result<(), String> {
    env_logger::init();

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|e| format!("无法获取用户主目录: {e}"))?;
    let db_path = std::path::PathBuf::from(home)
        .join(".filemind")
        .join("filemind.db");

    filemind_lib::db::Database::open(&db_path).map_err(|e| e.to_string())?;
    log::info!("database ready at {}", db_path.display());
    Ok(())
}
