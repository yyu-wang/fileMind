//! `ConfigRepo` 扩展：`cloud_providers` 表 CRUD（原 `commands/cloud_providers.rs` 拆出）。
//!
//! 放在命令模块而非 `config_repo.rs`：保持 `config_repo` 只负责 `app_config` 单行表，
//! 其它表按领域就近挂载。

use rusqlite::Connection;

use super::types::CloudProviderRecord;
use crate::db::ConfigRepo;
use crate::error::{AppError, AppResult};

impl ConfigRepo {
    /// 列出所有未软删除的云提供商（内置 → 最新更新时间 DESC）。
    ///
    /// # Errors
    /// 表尚未创建（开发环境）时返回空向量；其它查询错误按 `AppError` 回传。
    pub fn list_cloud_providers(conn: &Connection) -> AppResult<Vec<CloudProviderRecord>> {
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
        conn: &Connection,
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
            return Err(AppError::InvalidInput(format!(
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
    pub fn delete_cloud_provider(conn: &Connection, provider_key: &str) -> AppResult<()> {
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
        conn: &Connection,
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
        conn: &Connection,
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

fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
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
