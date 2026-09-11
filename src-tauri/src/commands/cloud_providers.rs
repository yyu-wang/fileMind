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

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::config::{validate_provider_key, CloudProvider};
use crate::db::ConfigRepo;
use crate::error::AppResult;
use crate::AppState;

/// 云提供商记录（`cloud_providers` 表的一行，不含 API Key——Key 只在 Keychain）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CloudProviderRecord {
    /// 记录主键（UUID 字符串）。
    pub id: String,
    /// 提供商标识 slug（`[a-z0-9_-]{1,64}`），全局唯一、软删除外仍唯一。
    pub provider_key: String,
    /// 显示名称（如「Claude 官方」「公司代理中转」）。
    pub name: String,
    /// 备注（纯 UI 展示，如「公司专用账号」）。
    pub remark: String,
    /// 官网链接（可选）。
    pub website: Option<String>,
    /// 请求地址前缀（OpenAI 兼容 `base_url`）。
    pub base_url: String,
    /// 是否为迁移时内置的两条模板（OpenAI/DeepSeek）。
    pub is_builtin: bool,
    /// 创建时间（ISO 8601 UTC）。
    pub created_at: String,
    /// 更新时间（ISO 8601 UTC）。
    pub updated_at: String,
}

/// 新建 / 更新提供商时的请求体（不含 id / 时间戳）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CloudProviderUpsertInput {
    /// 提供商标识（slug）；创建时必填，更新时用它定位到目标记录。
    pub provider_key: String,
    /// 显示名称（1-128 字符）。
    pub name: String,
    /// 备注（最长 255 字符，空串允许）。
    pub remark: Option<String>,
    /// 官网链接（必须 `https://` 或留空；`http://` 仅 `localhost`/`127.0.0.1`）。
    pub website: Option<String>,
    /// 请求地址前缀（必须是合法 URL，不能以 `/` 结尾）。
    pub base_url: String,
}

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

// ---------- 输入校验（P-07：防止表单脏值流入 DB / URL 转发） ----------

fn validate_display_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("PROV-110:供应商名称不能为空".to_string());
    }
    if trimmed.chars().count() > 128 {
        return Err("PROV-111:供应商名称不能超过 128 字符".to_string());
    }
    Ok(())
}

fn validate_remark(remark: &str) -> Result<(), String> {
    if remark.chars().count() > 255 {
        return Err("PROV-112:备注不能超过 255 字符".to_string());
    }
    Ok(())
}

/// 校验可选官网：空串/None → None；合法 https → Some(raw)；http 仅允许本地回环。
fn validate_optional_website(website: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = website else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        if let Some(rest_local) = trimmed.strip_prefix("http://") {
            // 本地代理开发调试用：只放行 localhost / 127.0.0.1 + 端口
            let host_part = rest_local.split_once('/').map_or(rest_local, |(h, _)| h);
            let host = host_part.split(':').next().unwrap_or(host_part);
            if host == "localhost" || host == "127.0.0.1" {
                return Ok(Some(trimmed.to_string()));
            }
        }
        return Err(
            "PROV-120:官网链接必须是 https://，仅本地调试允许 http://localhost".to_string(),
        );
    };
    if rest.is_empty() {
        return Err("PROV-121:官网链接缺少域名".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

/// 校验 `base_url`：必须 `https://host[/prefix]`，允许本地 http；不许 `/` 结尾。
fn validate_base_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return Err("PROV-130:请求地址不能为空".to_string());
    }
    if trimmed.ends_with('/') {
        return Err("PROV-131:请求地址不能以 '/' 结尾（提示：填写到域名+前缀即可）".to_string());
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        if let Some(rest_local) = trimmed.strip_prefix("http://") {
            let host_part = rest_local.split_once('/').map_or(rest_local, |(h, _)| h);
            let host = host_part.split(':').next().unwrap_or(host_part);
            if host != "localhost" && host != "127.0.0.1" {
                return Err(
                    "PROV-132:请求地址仅允许 https://，本地中转才可使用 http://localhost"
                        .to_string(),
                );
            }
            return Ok(trimmed);
        }
        return Err("PROV-133:请求地址必须是 https:// 开头的 URL".to_string());
    };
    if rest.is_empty() {
        return Err("PROV-134:请求地址缺少域名".to_string());
    }
    Ok(trimmed)
}

// ---------- ConfigRepo 扩展：cloud_providers 表 CRUD ----------
//
// 放在命令模块而非 config_repo.rs：保持 config_repo 只负责 app_config 单行表，
// 其它表按领域就近挂载。

impl ConfigRepo {
    /// 列出所有未软删除的云提供商（内置 → 最新更新时间 DESC）。
    ///
    /// # Errors
    /// 表尚未创建（开发环境）时返回空向量；其它查询错误按 `AppError` 回传。
    pub fn list_cloud_providers(
        conn: &rusqlite::Connection,
    ) -> AppResult<Vec<CloudProviderRecord>> {
        if !table_exists(conn, "cloud_providers")? {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT id, provider_key, name, remark, website, base_url, is_builtin, created_at, updated_at
             FROM cloud_providers
             WHERE is_deleted = 0
             ORDER BY is_builtin DESC, datetime(updated_at) DESC",
        )?;
        let rows = stmt.query_map([], map_provider_record)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 新增或覆盖写入云提供商。
    ///
    /// - 若 `provider_key` 存在且未删除 → 更新 `name`/`remark`/`website`/`base_url` + `updated_at`。
    /// - 若 `provider_key` 不存在 → 插入新行（生成 `UUID` `id` + `is_builtin=0`）。
    /// - 若 `provider_key` 已被软删除 → 返回 PROV-140 冲突错误，避免历史审计键复用。
    ///
    /// # Errors
    /// 表不存在会先尝试查询；任何 DB 层错误按 `AppResult` 抛出。
    pub fn upsert_cloud_provider(
        conn: &rusqlite::Connection,
        provider_key: &str,
        name: &str,
        remark: &str,
        website: Option<&str>,
        base_url: &str,
    ) -> AppResult<CloudProviderRecord> {
        // 1) 软删除冲突检测
        let deleted: bool = conn
            .query_row(
                "SELECT 1 FROM cloud_providers WHERE provider_key = ?1 AND is_deleted = 1 LIMIT 1",
                [provider_key],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if deleted {
            return Err(crate::error::AppError::InvalidInput(format!(
                "PROV-140:提供商标识 '{provider_key}' 已被删除，暂不允许复用（请换个 slug）"
            )));
        }
        // 2) 存在且未删除 → 更新
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM cloud_providers WHERE provider_key = ?1 AND is_deleted = 0 LIMIT 1",
                [provider_key],
                |_| Ok(true),
            )
            .unwrap_or(false);
        let now = now_iso8601();
        if exists {
            conn.execute(
                "UPDATE cloud_providers SET name=?1, remark=?2, website=?3, base_url=?4, updated_at=?5
                 WHERE provider_key=?6 AND is_deleted=0",
                rusqlite::params![name, remark, website, base_url, now, provider_key],
            )?;
        } else {
            let id = new_uuid();
            conn.execute(
                "INSERT INTO cloud_providers
                 (id, provider_key, name, remark, website, base_url, is_builtin, is_deleted, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7, ?8)",
                rusqlite::params![id, provider_key, name, remark, website, base_url, now, now],
            )?;
        }
        // 3) 回读写回的记录（以 DB 真值为准）
        let record = conn.query_row(
            "SELECT id, provider_key, name, remark, website, base_url, is_builtin, created_at, updated_at
             FROM cloud_providers WHERE provider_key=?1 AND is_deleted=0 LIMIT 1",
            [provider_key],
            map_provider_record,
        )?;
        Ok(record)
    }

    /// 软删除：按 `provider_key` 打 `is_deleted=1`。不存在的 key 视为成功（幂等）。
    ///
    /// # Errors
    ///
    /// 软删除语句执行失败，或表存在性检测失败时，返回 `AppError`。
    pub fn delete_cloud_provider(conn: &rusqlite::Connection, provider_key: &str) -> AppResult<()> {
        if !table_exists(conn, "cloud_providers")? {
            return Ok(());
        }
        let now = now_iso8601();
        conn.execute(
            "UPDATE cloud_providers SET is_deleted = 1, updated_at = ?1 WHERE provider_key = ?2 AND is_deleted = 0",
            rusqlite::params![now, provider_key],
        )?;
        Ok(())
    }

    /// 按 `provider_key` 查记录；未找到 / 已删除 / 表未建 → `None`。
    ///
    /// # Errors
    ///
    /// DB 查询失败，或表存在性检测失败时，返回 `AppError`。
    pub fn get_cloud_provider(
        conn: &rusqlite::Connection,
        provider_key: &str,
    ) -> AppResult<Option<CloudProviderRecord>> {
        if !table_exists(conn, "cloud_providers")? {
            return Ok(None);
        }
        let result = conn.query_row(
            "SELECT id, provider_key, name, remark, website, base_url, is_builtin, created_at, updated_at
             FROM cloud_providers WHERE provider_key=?1 AND is_deleted=0 LIMIT 1",
            [provider_key],
            map_provider_record,
        );
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 查询激活提供商的上游 `base_url`（供 `Cloud Proxy` 转发使用）。
    ///
    /// 返回 `Some(base_url)` 仅当表存在、记录存在且未删除；否则 `None`。
    ///
    /// # Errors
    ///
    /// DB 查询失败，或表存在性检测失败时，返回 `AppError`。
    pub fn get_provider_base_url(
        conn: &rusqlite::Connection,
        provider_key: &str,
    ) -> AppResult<Option<String>> {
        if !table_exists(conn, "cloud_providers")? {
            return Ok(None);
        }
        let result = conn.query_row(
            "SELECT base_url FROM cloud_providers WHERE provider_key=?1 AND is_deleted=0 LIMIT 1",
            [provider_key],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(u) => Ok(Some(u)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

fn map_provider_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<CloudProviderRecord> {
    let is_builtin_int: i64 = row.get(6)?;
    Ok(CloudProviderRecord {
        id: row.get(0)?,
        provider_key: row.get(1)?,
        name: row.get(2)?,
        remark: row.get(3)?,
        website: row.get(4)?,
        base_url: row.get(5)?,
        is_builtin: is_builtin_int != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn new_uuid() -> String {
    // 无外部 UUID v4 序列化需求：直接用项目已依赖的 uuid crate 生成，
    // 与迁移脚本 `lower(hex(randomblob(16)))` 生成格式一致（32 位小写 hex）。
    uuid::Uuid::new_v4().as_simple().to_string()
}

fn now_iso8601() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::db::Database;
    use tempfile::NamedTempFile;

    fn open_test_db() -> Database {
        let tmp = NamedTempFile::new().expect("临时文件创建失败");
        Database::open(tmp.path()).expect("数据库打开失败")
    }

    #[test]
    fn validate_base_url_accepts_https_and_rejects_trailing_slash() {
        assert_eq!(
            validate_base_url("https://api.deepseek.com").unwrap(),
            "https://api.deepseek.com"
        );
        assert!(validate_base_url("https://api.example.com/v1").is_ok());
        assert!(validate_base_url("http://localhost:11434/v1").is_ok());
        assert!(validate_base_url("https://open.bigmodel.cn/api/paas/v4/").is_err()); // 尾斜杠
        assert!(validate_base_url("http://evil.com").is_err()); // 非本地 http
        assert!(validate_base_url("").is_err());
        assert!(validate_base_url("ftp://x").is_err());
    }

    #[test]
    fn validate_website_allows_localhost_http_only() {
        assert!(validate_optional_website(Some("https://www.anthropic.com")).is_ok());
        assert!(validate_optional_website(Some("http://localhost:8080")).is_ok());
        assert!(validate_optional_website(Some("http://127.0.0.1:8080")).is_ok());
        assert!(validate_optional_website(Some("http://other.com")).is_err());
        assert!(validate_optional_website(Some("")).is_ok()); // 空当 None
        assert!(validate_optional_website(None).is_ok());
    }

    #[test]
    fn list_seeded_returns_two_builtins() {
        let db = open_test_db();
        let list = ConfigRepo::list_cloud_providers(db.conn()).unwrap();
        assert_eq!(list.len(), 2, "迁移 V015 预置两条内建记录");
        let keys: Vec<&str> = list.iter().map(|r| r.provider_key.as_str()).collect();
        assert!(keys.contains(&"openai"));
        assert!(keys.contains(&"deepseek"));
    }

    #[test]
    fn upsert_inserts_then_updates_same_key() {
        let db = open_test_db();
        let created = ConfigRepo::upsert_cloud_provider(
            db.conn(),
            "silicon-flow",
            "硅基流动",
            "国内代理",
            None,
            "https://api.siliconflow.cn",
        )
        .unwrap();
        assert_eq!(created.provider_key, "silicon-flow");
        assert_eq!(created.name, "硅基流动");
        assert!(!created.is_builtin);

        let updated = ConfigRepo::upsert_cloud_provider(
            db.conn(),
            "silicon-flow",
            "硅基流动 v2",
            "国内代理·高优先级",
            None,
            "https://api.siliconflow.cn/v1",
        )
        .unwrap();
        assert_eq!(updated.name, "硅基流动 v2");
        assert_eq!(updated.remark, "国内代理·高优先级");
        assert_eq!(updated.base_url, "https://api.siliconflow.cn/v1");
    }

    #[test]
    fn delete_is_soft_idempotent_and_conflicts_on_reinsert() {
        let db = open_test_db();
        // 先新增一条自定义
        ConfigRepo::upsert_cloud_provider(
            db.conn(),
            "mars",
            "火星模型",
            "",
            None,
            "https://mars.example.com",
        )
        .unwrap();
        ConfigRepo::delete_cloud_provider(db.conn(), "mars").unwrap();
        // 第二次删幂等
        ConfigRepo::delete_cloud_provider(db.conn(), "mars").unwrap();
        // 列表只剩预置
        let list = ConfigRepo::list_cloud_providers(db.conn()).unwrap();
        assert!(list.iter().all(|r| r.provider_key != "mars"));
        // 同名再插报错
        let result = ConfigRepo::upsert_cloud_provider(
            db.conn(),
            "mars",
            "火星模型 复用",
            "",
            None,
            "https://mars.example.com",
        );
        assert!(result.is_err(), "软删除后的 key 再 upsert 应返回冲突错误");
    }

    #[test]
    fn get_provider_base_url_matches_seeded() {
        let db = open_test_db();
        let deepseek = ConfigRepo::get_provider_base_url(db.conn(), "deepseek")
            .unwrap()
            .expect("预置 deepseek 必存在");
        assert_eq!(deepseek, "https://api.deepseek.com");
        let missing = ConfigRepo::get_provider_base_url(db.conn(), "unknown-xxx").unwrap();
        assert!(missing.is_none());
    }
}

// lint fix notes: doc_markdown (base_url / AppError / AppResult / name remark website base_url updated_at UUID id is_builtin=0 Cloud Proxy 反引号),
// missing_errors_doc (delete_cloud_provider / get_cloud_provider / get_provider_base_url 加 # Errors 段落)
