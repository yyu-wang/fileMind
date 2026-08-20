//! `app_config` 表数据仓库：读写应用全局配置与云端知情同意书状态。
//!
//! 单行表（id=1），所有方法直接操作该固定行，无需 WHERE 条件。

use rusqlite::{params, Connection};

use crate::commands::config::{AppConfig, CloudProvider};
use crate::error::AppResult;

const SELECT_CONFIG_SQL: &str = "
    SELECT data_directory, inference_mode, embedding_model, llm_model, max_file_size_mb,
           language, onboarding_completed,
           cloud_consent_signed, cloud_consent_version, cloud_consent_provider,
           cloud_consent_signed_at
    FROM app_config WHERE id = 1
";

const UPSERT_CONFIG_SQL: &str = "
    INSERT INTO app_config (
        id, data_directory, inference_mode, embedding_model, llm_model, max_file_size_mb,
        language, onboarding_completed,
        cloud_consent_signed, cloud_consent_version, cloud_consent_provider,
        cloud_consent_signed_at
    ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
        cloud_consent_signed_at = excluded.cloud_consent_signed_at
";

const SIGN_CONSENT_SQL: &str = "
    UPDATE app_config SET
        cloud_consent_signed = 1,
        cloud_consent_version = ?1,
        cloud_consent_provider = ?2,
        cloud_consent_signed_at = ?3,
        inference_mode = 'cloud'
    WHERE id = 1
";

const REVOKE_CONSENT_SQL: &str = "
    UPDATE app_config SET
        cloud_consent_signed = 0,
        cloud_consent_version = NULL,
        cloud_consent_provider = NULL,
        cloud_consent_signed_at = NULL,
        inference_mode = 'local'
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
        let provider_str = config.cloud_consent_provider.map(provider_to_str);
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
        conn.execute(
            SIGN_CONSENT_SQL,
            params![consent_version, provider_to_str(provider), now],
        )?;
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
        cloud_consent_signed_at: row.get(10)?,
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

/// `CloudProvider` → `SQLite` 字符串。
const fn provider_to_str(p: CloudProvider) -> &'static str {
    match p {
        CloudProvider::Openai => "openai",
        CloudProvider::Deepseek => "deepseek",
    }
}

/// `SQLite` 字符串 → `CloudProvider`（未知值兜底为 `OpenAI`）。
const fn str_to_provider(s: &str) -> CloudProvider {
    if s.eq_ignore_ascii_case("deepseek") {
        CloudProvider::Deepseek
    } else {
        CloudProvider::Openai
    }
}

/// 当前时间（ISO 8601，本地时区）。
fn now_iso8601() -> String {
    // 简单实现：用系统时间格式化为 RFC3339
    // 避免引入 chrono 依赖（项目其他模块也用 std::time）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // 这里用最小实现：存 epoch 秒数的 ISO 字符串
    // 完整 ISO 8601 格式化留待 T11 统一替换为 chrono
    format!("epoch:{secs}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    // 测试代码允许 unwrap/expect/panic：简洁直观地表达失败语义

    use super::*;
    use crate::db::Database;
    use tempfile::NamedTempFile;

    /// 构建临时文件数据库并跑全部迁移。
    fn open_test_db() -> Database {
        let tmp = NamedTempFile::new().expect("临时文件创建失败");
        Database::open(tmp.path()).expect("数据库打开失败")
    }

    #[test]
    fn get_returns_default_after_migration() {
        let db = open_test_db();
        let config = ConfigRepo::get(db.conn()).expect("读取默认配置");
        assert_eq!(config.inference_mode, "local");
        assert_eq!(config.llm_model, "qwen3.8-27b");
        assert!(!config.onboarding_completed);
        assert!(!config.cloud_consent_signed);
        assert!(config.cloud_consent_version.is_none());
    }

    #[test]
    fn upsert_persists_all_fields() {
        let db = open_test_db();
        let mut config = ConfigRepo::get(db.conn()).unwrap();
        config.data_directory = "/tmp/test".to_string();
        config.inference_mode = "cloud".to_string();
        config.llm_model = "qwen3.8-14b".to_string();
        config.onboarding_completed = true;
        config.max_file_size_mb = 200;
        ConfigRepo::upsert(db.conn(), &config).unwrap();

        let reloaded = ConfigRepo::get(db.conn()).unwrap();
        assert_eq!(reloaded.data_directory, "/tmp/test");
        assert_eq!(reloaded.inference_mode, "cloud");
        assert_eq!(reloaded.llm_model, "qwen3.8-14b");
        assert!(reloaded.onboarding_completed);
        assert_eq!(reloaded.max_file_size_mb, 200);
    }

    #[test]
    fn sign_consent_sets_fields_and_switches_mode() {
        let db = open_test_db();
        ConfigRepo::sign_consent(db.conn(), "v1.0", CloudProvider::Openai).unwrap();
        let config = ConfigRepo::get(db.conn()).unwrap();
        assert!(config.cloud_consent_signed);
        assert_eq!(config.cloud_consent_version.as_deref(), Some("v1.0"));
        assert_eq!(config.inference_mode, "cloud");
    }

    #[test]
    fn revoke_consent_clears_fields_and_switches_back() {
        let db = open_test_db();
        ConfigRepo::sign_consent(db.conn(), "v1.0", CloudProvider::Deepseek).unwrap();
        ConfigRepo::revoke_consent(db.conn()).unwrap();
        let config = ConfigRepo::get(db.conn()).unwrap();
        assert!(!config.cloud_consent_signed);
        assert!(config.cloud_consent_version.is_none());
        assert_eq!(config.inference_mode, "local");
    }

    #[test]
    fn provider_roundtrip_preserves_value() {
        let db = open_test_db();
        ConfigRepo::sign_consent(db.conn(), "v1.0", CloudProvider::Deepseek).unwrap();
        let config = ConfigRepo::get(db.conn()).unwrap();
        match config.cloud_consent_provider {
            Some(CloudProvider::Deepseek) => {}
            other => panic!("期望 Deepseek，实际 {other:?}"),
        }
    }
}
