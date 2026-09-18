//! `app_config` 表数据仓库：读写应用全局配置与云端知情同意书状态。
//!
//! 单行表（id=1），所有方法直接操作该固定行，无需 WHERE 条件。

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::commands::config::{AppConfig, CloudProvider};
use crate::error::AppResult;

const SELECT_CONFIG_SQL: &str = "
    SELECT data_directory, inference_mode, embedding_model, llm_model, max_file_size_mb,
           language, onboarding_completed,
           cloud_consent_signed, cloud_consent_version, cloud_consent_provider,
           cloud_consent_signed_at,
           cloud_model,
           active_cloud_provider,
           local_llm_backend, local_llm_model
    FROM app_config WHERE id = 1
";

const UPSERT_CONFIG_SQL: &str = "
    INSERT INTO app_config (
        id, data_directory, inference_mode, embedding_model, llm_model, max_file_size_mb,
        language, onboarding_completed,
        cloud_consent_signed, cloud_consent_version, cloud_consent_provider,
        cloud_consent_signed_at,
        cloud_model,
        active_cloud_provider,
        local_llm_backend, local_llm_model
    ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
    ON CONFLICT(id) DO UPDATE SET
        data_directory = excluded.data_directory,
        inference_mode = excluded.inference_mode,
        embedding_model = excluded.embedding_model,
        llm_model = excluded.llm_model,
        max_file_size_mb = excluded.max_file_size_mb,
        language = excluded.language,
        onboarding_completed = excluded.onboarding_completed,
        cloud_consent_signed = excluded.cloud_consent_signed,
        cloud_consent_version = excluded.cloud_consent_version,
        cloud_consent_provider = excluded.cloud_consent_provider,
        cloud_consent_signed_at = excluded.cloud_consent_signed_at,
        cloud_model = excluded.cloud_model,
        active_cloud_provider = excluded.active_cloud_provider,
        local_llm_backend = excluded.local_llm_backend,
        local_llm_model = excluded.local_llm_model
";

const SIGN_CONSENT_SQL: &str = "
    UPDATE app_config SET
        cloud_consent_signed = 1,
        cloud_consent_version = ?1,
        cloud_consent_provider = ?2,
        cloud_consent_signed_at = ?3,
        inference_mode = 'cloud',
        active_cloud_provider = ?2
    WHERE id = 1
";

const REVOKE_CONSENT_SQL: &str = "
    UPDATE app_config SET
        cloud_consent_signed = 0,
        cloud_consent_version = NULL,
        cloud_consent_provider = NULL,
        cloud_consent_signed_at = NULL,
        inference_mode = 'local',
        active_cloud_provider = NULL
    WHERE id = 1
";

/// `app_config` 表仓库。
pub struct ConfigRepo;

impl ConfigRepo {
    /// 读取应用配置（id=1 单行）。
    ///
    /// # Errors
    ///
    /// 查询或行映射失败时返回错误。
    pub fn get(conn: &Connection) -> AppResult<AppConfig> {
        conn.query_row(SELECT_CONFIG_SQL, [], map_config)
            .map_err(Into::into)
    }

    /// 写入或更新应用配置（id=1 单行，UPSERT 语义）。
    ///
    /// # Errors
    ///
    /// 写入失败时返回数据库错误。
    pub fn upsert(conn: &Connection, config: &AppConfig) -> AppResult<()> {
        let provider_str = config
            .cloud_consent_provider
            .as_deref()
            .map(str_to_provider);
        // u64 → i64：max_file_size_mb 实际值域远小于 i64 正数范围，不会 wrap
        #[allow(clippy::cast_possible_wrap)]
        let max_file_size = config.max_file_size_mb as i64;
        conn.execute(
            UPSERT_CONFIG_SQL,
            params![
                config.data_directory,
                config.inference_mode,
                config.embedding_model,
                config.llm_model,
                max_file_size,
                config.language,
                bool_to_int(config.onboarding_completed),
                bool_to_int(config.cloud_consent_signed),
                config.cloud_consent_version,
                provider_str,
                config.cloud_consent_signed_at,
                config.cloud_model,
                config.active_cloud_provider,
                config.local_llm_backend,
                config.local_llm_model,
            ],
        )?;
        Ok(())
    }

    /// 签署云端知情同意书并自动切到 `cloud` 模式（04 API §2-3c）。
    ///
    /// # Errors
    ///
    /// 更新失败时返回数据库错误。
    pub fn sign_consent(
        conn: &Connection,
        consent_version: &str,
        provider: CloudProvider,
    ) -> AppResult<()> {
        let now = now_iso8601();
        conn.execute(SIGN_CONSENT_SQL, params![consent_version, provider, now])?;
        Ok(())
    }

    /// 撤回云端知情同意书并自动切回 `local` 模式（04 API §2-3d 联动）。
    ///
    /// # Errors
    ///
    /// 更新失败时返回数据库错误。
    pub fn revoke_consent(conn: &Connection) -> AppResult<()> {
        conn.execute(REVOKE_CONSENT_SQL, [])?;
        Ok(())
    }
}

/// 把一行数据映射为 `AppConfig`。
///
/// `max_file_size_mb` 字段：`SQLite` 不支持 u64，存读 i64，值域远小于 i64 正数范围不会 wrap。
#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn map_config(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppConfig> {
    let onboarding_int: i64 = row.get(6)?;
    let consent_int: i64 = row.get(7)?;
    let provider_str: Option<String> = row.get(9)?;
    let active_str: Option<String> = row.get(12)?;
    Ok(AppConfig {
        data_directory: row.get(0)?,
        inference_mode: row.get(1)?,
        embedding_model: row.get(2)?,
        llm_model: row.get(3)?,
        max_file_size_mb: row.get::<_, i64>(4)? as u64,
        language: row.get(5)?,
        onboarding_completed: int_to_bool(onboarding_int),
        cloud_consent_signed: int_to_bool(consent_int),
        cloud_consent_version: row.get(8)?,
        cloud_consent_provider: provider_str.as_deref().map(str_to_provider),
        // 旧版 epoch: 前缀兼容：读取时归一化为 RFC3339（BE-M2）
        cloud_consent_signed_at: row.get::<_, Option<String>>(10)?.map(normalize_signed_at),
        cloud_model: row.get(11)?,
        active_cloud_provider: active_str,
        local_llm_backend: row.get(13)?,
        local_llm_model: row.get(14)?,
    })
}

/// `SQLite` INTEGER 0/1 → bool（06 规范：`SQLite` boolean 存为 INTEGER）。
const fn int_to_bool(v: i64) -> bool {
    v != 0
}

/// bool → `SQLite` INTEGER 0/1。
///
/// `clippy::bool_to_int_with_if` 与 `clippy::match_bool` 互相冲突，const fn 中无法用
/// `i64::from(v)`（unstable），此处用 if/else 是最简实现。
#[allow(clippy::bool_to_int_with_if)]
const fn bool_to_int(v: bool) -> i64 {
    if v {
        1
    } else {
        0
    }
}

/// `SQLite` 字符串 → `CloudProvider`（String 直读，空值 None 由上层处理；
/// 保留独立函数以保持与写入路径的对称、方便后续加规范化）。
fn str_to_provider(s: &str) -> CloudProvider {
    s.to_string()
}

/// 当前时间（RFC3339 UTC，如 `2026-08-24T12:34:56Z`）。
///
/// 前端 `new Date()` 可直接解析；与 `operations_log` 的时间语义一致（均 UTC）。
fn now_iso8601() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 兼容旧版 `epoch:` 前缀时间戳（BE-M2）：读取时转换为 RFC3339。
///
/// 历史版本 `now_iso8601` 写入的是 `epoch:{秒}`，破坏了 `cloud_consent_signed_at`
/// 的 ISO 8601 字段契约。读取时识别前缀转换为新格式；下次 upsert 自然写回
/// RFC3339 原值（自愈）。解析失败的旧值原样保留，不 panic、不丢数据。
fn normalize_signed_at(s: String) -> String {
    if let Some(converted) = s
        .strip_prefix("epoch:")
        .and_then(|secs| secs.parse::<i64>().ok())
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    {
        return converted;
    }
    s
}

// 单元测试移到兄弟文件 config_repo_tests.rs（原内嵌，与实现合计 385 行）。

#[cfg(test)]
#[path = "config_repo_tests.rs"]
mod tests;
