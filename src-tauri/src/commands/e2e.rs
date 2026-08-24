//! (T9.5) E2E 测试专用命令：仅 debug 构建编译并注册，release 不携带自动化入口。
//!
//! 安全：命令仅回读 `FILEMIND_E2E_DATA_DIR` env，不触碰任何真实用户数据；
//! env 未设置时返回 `None`，前端据此回退到原生目录对话框，不改变生产路径。

/// 返回 E2E 测试文件目录（读取 `FILEMIND_E2E_DATA_DIR`）。
///
/// 由前端 `src/lib/e2e.ts` 在挂载时调用；release 未注册此命令时
/// `invoke` 抛错 → 前端 try/catch 返回 `None` → 走原生对话框。
#[must_use]
#[tauri::command]
#[specta::specta]
pub fn e2e_get_test_dir() -> Option<String> {
    std::env::var("FILEMIND_E2E_DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
}
