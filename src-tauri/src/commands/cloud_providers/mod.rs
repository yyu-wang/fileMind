//! 用户自定义云提供商管理命令（P-07 自定义提供商改造）。
//!
//! 对应 SQLite `cloud_providers` 表：前端设置页的提供商表单就是本表的增删改查。
//! `base_url` 字段是云端代理转发的**真实上游地址前缀**（如
//! `https://api.deepseek.com`），代理在收到 Sidecar 请求时按 `provider_key` 拼
//! `/chat/completions` 后转发——安全保证：Python 永远不直接持有这个 URL，
//! 代理侧查 DB 读取，杜绝 Sidecar 通过 env 注入恶意上游。
//!
//! 幂等语义：`upsert_cloud_provider` 按 `provider_key` 做唯一键插入或更新；
//! 「删除」走软删除（`is_deleted=1`），历史审计与 Keychain 条目保留，用户若
//! 再同名新建会冲突（前端提示），避免「删了立刻重加」的丢失语义歧义。
//!
//! 模块划分（原单文件 412 行，逼近 Rust 模块 500 行强制阈值，见 `rules/complexity.md`）：
//!   - `validate` 输入校验（防表单脏值流入 DB / URL 转发）
//!   - `repo`     `ConfigRepo` 上挂载的 `cloud_providers` 表 CRUD
//!   - `types`    对外 IPC 契约（specta 导出）
//!
//! ⚠️ 三个 `#[tauri::command]` 留在本模块（不做再导出）：`#[tauri::command]` /
//! `#[specta::specta]` 生成的隐藏辅助项（`__cmd__*` / `__specta__fn__*`）只存在于命令
//! 定义所在模块，宏是按「给定路径的父模块」去找它们的，`pub use` 不会把辅助项带过来
//! （同 `commands/file_ops/mod.rs` 的说明），故 `ipc_handler.rs` /
//! `bin/export_specta.rs` 的注册路径保持 `commands::cloud_providers::xxx` 不变。

use tauri::State;

use self::validate::{
    validate_base_url, validate_display_name, validate_optional_website, validate_remark,
};
// provider_key 的 slug 校验与配置表共用一套规则（`commands::config`）；
// `cloud_providers` 表 CRUD 挂在 ConfigRepo 上（见 `self::repo`）
use crate::commands::config::{validate_provider_key, CloudProvider};
use crate::db::ConfigRepo;
use crate::AppState;

mod repo;
mod types;
mod validate;

// 对外 IPC 契约的再导出（命令签名与本模块内使用共用这一处引入）
pub use types::{CloudProviderRecord, CloudProviderUpsertInput};

/// 列出所有未软删除的云提供商。
///
/// 排序：内置优先，其余按 `updated_at DESC`。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn list_cloud_providers(
    state: State<'_, AppState>,
) -> Result<Vec<CloudProviderRecord>, String> {
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB-CP-001:数据库读取失败 ({e})"))?;
    ConfigRepo::list_cloud_providers(db.conn())
        .map_err(|e| format!("DB-CP-002:提供商列表读取失败 ({e})"))
}

/// 新增或按 `provider_key` 更新云提供商。
///
/// 返回写回的完整记录（含 id/时间戳）。
///
/// # Errors
///
/// `provider_key` 不符合 slug 规则，或 `base_url`/`website` 格式非法，或
/// 软删除记录同名重建（slug 冲突），或写入 DB 失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn upsert_cloud_provider(
    state: State<'_, AppState>,
    input: CloudProviderUpsertInput,
) -> Result<CloudProviderRecord, String> {
    validate_provider_key(&input.provider_key)?;
    validate_display_name(&input.name)?;
    let remark = input.remark.unwrap_or_default();
    validate_remark(&remark)?;
    let website = validate_optional_website(input.website.as_deref())?;
    let base_url = validate_base_url(&input.base_url)?;

    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB-CP-001:数据库读取失败 ({e})"))?;
    ConfigRepo::upsert_cloud_provider(
        db.conn(),
        &input.provider_key,
        &input.name,
        &remark,
        website.as_deref(),
        &base_url,
    )
    .map_err(|e| format!("DB-CP-003:提供商保存失败 ({e})"))
}

/// 按 `provider_key` 软删除云提供商（不删 Keychain，用户可手动再删 Key）。
///
/// # Errors
///
/// 提供商标识非法、DB 锁或执行失败时返回错误；不存在的 `provider_key` 返回成功
/// （幂等）。
#[tauri::command(async)]
#[specta::specta]
pub fn delete_cloud_provider(
    state: State<'_, AppState>,
    provider_key: CloudProvider,
) -> Result<(), String> {
    validate_provider_key(&provider_key)?;
    let db = state
        .db
        .lock()
        .map_err(|e| format!("DB-CP-001:数据库读取失败 ({e})"))?;
    ConfigRepo::delete_cloud_provider(db.conn(), &provider_key)
        .map_err(|e| format!("DB-CP-004:提供商删除失败 ({e})"))
}

#[cfg(test)]
#[path = "../cloud_providers_tests.rs"]
mod tests;

// lint fix notes: doc_markdown (base_url / AppError / AppResult / name remark website base_url updated_at UUID id is_builtin=0 Cloud Proxy 反引号),
// missing_errors_doc (delete_cloud_provider / get_cloud_provider / get_provider_base_url 加 # Errors 段落)
