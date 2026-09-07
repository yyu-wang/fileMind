//! `files` 表数据仓库：批量写入、查询、分类更新与软删除。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};

const INSERT_FILE_SQL: &str = "
    INSERT INTO files (id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, datetime('now'), datetime('now'), ?7)
    ON CONFLICT(path) DO UPDATE SET
        file_name = excluded.file_name,
        file_size = excluded.file_size,
        content_hash = excluded.content_hash,
        is_deleted = 0,
        updated_at = datetime('now'),
        mtime = excluded.mtime
";

const GET_BY_PATH_SQL: &str = "
    SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
    FROM files WHERE path = ?1 AND is_deleted = 0
";

/// 单条 SQL 的 `IN (...)` 分块阈值。SQLite 绑定变量上限为 999，留安全余量取 900
/// （T10.1：十万文件扫描时逐条反查是主要瓶颈，改为 900/批的批量快照）。
const CHUNK_IN_PATHS: usize = 900;

/// 增量扫描快照：磁盘元信息与库内既有记录的比对基准（T10.1）。
///
/// 供 [`FileRepo::load_snapshots_by_paths`] 加载后，在锁外做
/// 「(size, mtime) 未变 → 复用 `content_hash`、跳过重算」的增量判定。
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// 库内既有 id（`upsert_batch` 需回写 `id_map`，避免扫描生成的新 uuid 与旧 id 不一致）。
    pub id: String,
    /// 软删除标记（软删记录需复活，不能直接跳过）。
    pub is_deleted: bool,
    /// 既有内容哈希（`None` 表示未计算过，需重算）。
    pub content_hash: Option<String>,
    /// 既有文件大小（字节）。
    pub file_size: i64,
    /// 既有磁盘修改时间（UTC `YYYY-MM-DD HH:MM:SS`；旧迁移行可能为 `None`）。
    pub mtime: Option<String>,
}

/// `files` 表仓库：全部方法接收外部连接，便于事务组合。
pub struct FileRepo;

impl FileRepo {
    /// 在单个事务中批量插入文件记录。
    ///
    /// # Errors
    ///
    /// 事务开启、插入或提交失败时返回错误。
    pub fn insert_batch(conn: &Connection, files: &[FileRecord]) -> AppResult<usize> {
        let tx = conn.unchecked_transaction()?;
        let mut count = 0;

        for file in files {
            tx.execute(
                INSERT_FILE_SQL,
                params![
                    file.id,
                    file.path,
                    file.file_name,
                    file.file_size,
                    file.content_hash,
                    file.category,
                    file.mtime,
                ],
            )?;
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    /// 按路径查找未删除文件。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误；不存在时返回 `None`。
    pub fn get_by_path(conn: &Connection, path: &str) -> AppResult<Option<FileRecord>> {
        let mut stmt = conn.prepare(GET_BY_PATH_SQL)?;
        let result = stmt.query_row(params![path], map_file_record);

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// 按 ID 查找未删除文件。
    ///
    /// # Errors
    ///
    /// 查询失败时返回错误；不存在时返回 `None`。
    pub fn get_by_id(conn: &Connection, id: &str) -> AppResult<Option<FileRecord>> {
        let result = conn.query_row(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE id = ?1 AND is_deleted = 0",
            params![id],
            map_file_record,
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// 批量按 ID 反查文件记录（`preview_operations` 用）。
    ///
    /// 行为：
    ///   - 顺序保证返回顺序与 `ids` 输入顺序一致（`SQL IN (...)` 不保证顺序，
    ///     这里用 `HashMap<id, FileRecord>` 重排）
    ///   - 不存在的 ID 静默跳过（调用方通过 `result.len()` vs `ids.len()` 判断丢失）
    ///   - 已软删除的记录（`is_deleted=1`）也跳过
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn get_by_ids(conn: &Connection, ids: &[String]) -> AppResult<Vec<FileRecord>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE id IN ({placeholders}) AND is_deleted = 0"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), map_file_record)?;

        // SQL IN 不保证顺序，按 ids 顺序重排
        let mut by_id: std::collections::HashMap<String, FileRecord> =
            std::collections::HashMap::new();
        for row in rows {
            let r = row?;
            by_id.insert(r.id.clone(), r);
        }
        let result = ids.iter().filter_map(|id| by_id.remove(id)).collect();
        Ok(result)
    }

    /// 分页列出未删除文件，可按分类过滤，按更新时间倒序。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn list(
        conn: &Connection,
        category: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let sql = match category {
            Some(_) => {
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
                 FROM files WHERE is_deleted = 0 AND category = ?1
                 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"
            }
            None => {
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
                 FROM files WHERE is_deleted = 0
                 ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2"
            }
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = match category {
            Some(cat) => stmt.query_map(params![cat, limit, offset], map_file_record)?,
            None => stmt.query_map(params![limit, offset], map_file_record)?,
        };

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    /// 统计未删除文件数量，可按分类过滤。
    ///
    /// # Errors
    ///
    /// 聚合查询失败时返回错误。
    pub fn count(conn: &Connection, category: Option<&str>) -> AppResult<i64> {
        let count: i64 = match category {
            Some(cat) => conn.query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND category = ?1",
                params![cat],
                |row| row.get(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )?,
        };
        Ok(count)
    }

    /// 更新指定文件的分类。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn update_category(conn: &Connection, id: &str, category: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET category = ?1, updated_at = datetime('now') WHERE id = ?2 AND is_deleted = 0",
            params![category, id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 移动/重命名后更新文件路径 + `updated_at`（`execute_operations` 用）。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn update_path(conn: &Connection, id: &str, new_path: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET path = ?1, file_name = ?2, updated_at = datetime('now')
             WHERE id = ?3 AND is_deleted = 0",
            params![
                new_path,
                Path::new(new_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                id
            ],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 批量增量写入：内容哈希未变化的跳过，变化的更新，新路径插入。
    ///
    /// 内部先批量加载快照（一条 `IN (...)` 查询替代逐条 `SELECT`，T10.1），
    /// 再交给 [`Self::upsert_batch_with_snapshots`] 决策。调用方若已在短锁阶段
    /// 预载过快照（`scan_directory`），应直接调 `with_snapshots` 避免二次查询。
    ///
    /// # Errors
    ///
    /// 事务开启、查询、写入或提交失败时返回错误。
    pub fn upsert_batch(conn: &Connection, files: &[FileRecord]) -> AppResult<UpsertResult> {
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let snapshots = Self::load_snapshots_by_paths(conn, &paths)?;
        Self::upsert_batch_with_snapshots(conn, files, &snapshots)
    }

    /// 批量增量写入（快照预载版）：`snapshots` 必须已由 [`Self::load_snapshots_by_paths`]
    /// 加载并覆盖 `files` 的全部路径。
    ///
    /// 跳过判定（T10.1 增量扫描核心）：
    ///   - 未软删、`content_hash` 相同、且 `(size, mtime)` 均一致 → `skipped`（不写库）
    ///   - `mtime` 为 `None` 的调用方（非扫描路径，如分类/恢复）退化为仅按 hash 比较，
    ///     保持既有跳过语义；扫描路径 `mtime` 恒有值，可精确跳过未变化文件
    ///   - 变化/新路径 → `INSERT ... ON CONFLICT(path)` 刷新元数据、复活软删记录并保留旧 id
    ///
    /// # Errors
    ///
    /// 事务开启、写入或提交失败时返回错误。
    pub fn upsert_batch_with_snapshots(
        conn: &Connection,
        files: &[FileRecord],
        snapshots: &HashMap<String, FileSnapshot>,
    ) -> AppResult<UpsertResult> {
        let tx = conn.unchecked_transaction()?;
        let mut result = UpsertResult::default();

        for file in files {
            if let Some(snapshot) = snapshots.get(&file.path) {
                // 已存在记录（含软删除行）：必须回写 id_map（ON CONFLICT 保留旧 id，
                // 扫描生成的新 id 若不回写，后续按 id 反查会落空）
                result.id_map.insert(file.id.clone(), snapshot.id.clone());
                let mtime_matches = file.mtime.is_none()
                    || (file.file_size == snapshot.file_size
                        && file.mtime.as_deref() == snapshot.mtime.as_deref());
                let unchanged = !snapshot.is_deleted
                    && file.content_hash.as_ref() == snapshot.content_hash.as_ref()
                    && mtime_matches;
                if unchanged {
                    result.skipped += 1;
                } else {
                    // 路径已存在但内容/元信息/软删状态变化：刷新并复活记录（保留旧 id）
                    tx.execute(
                        INSERT_FILE_SQL,
                        params![
                            file.id,
                            file.path,
                            file.file_name,
                            file.file_size,
                            file.content_hash,
                            file.category,
                            file.mtime,
                        ],
                    )?;
                    result.updated += 1;
                }
            } else {
                tx.execute(
                    INSERT_FILE_SQL,
                    params![
                        file.id,
                        file.path,
                        file.file_name,
                        file.file_size,
                        file.content_hash,
                        file.category,
                        file.mtime,
                    ],
                )?;
                result.id_map.insert(file.id.clone(), file.id.clone());
                result.added += 1;
            }
        }

        tx.commit()?;
        Ok(result)
    }

    /// 软删除指定文件（置 `is_deleted` 标记）。
    ///
    /// # Errors
    ///
    /// 文件不存在时返回 `QueryReturnedNoRows`；更新失败返回数据库错误。
    pub fn soft_delete(conn: &Connection, id: &str) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET is_deleted = 1, category = NULL, updated_at = datetime('now')
             WHERE id = ?1",
            params![id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 按路径前缀批量软删除（目录级移除用）。
    ///
    /// 匹配规则 `path LIKE prefix || '/%'`，避免 `/a/b` 误匹配 `/a/bc`。
    /// 返回受影响的行数；无匹配时返回 0（不报错，幂等）。
    ///
    /// 语义：目录级移除 = 放弃 `FileMind` 对该目录的全部派生状态。因此连同
    /// `category` 分类标签一起清空——否则重新扫描该目录时 `ON CONFLICT(path)`
    /// 复活记录会保留旧标签，文件明明还在原地却显示「已分类」，永远不再被
    /// 「未整理」类整理入口处理。`content_hash` 保留（增量扫描可复用，无副作用）。
    ///
    /// # Errors
    ///
    /// 更新失败时返回数据库错误。
    pub fn soft_delete_by_path_prefix(conn: &Connection, path_prefix: &str) -> AppResult<i64> {
        let affected = conn.execute(
            "UPDATE files SET is_deleted = 1, category = NULL, updated_at = datetime('now')
             WHERE is_deleted = 0 AND path LIKE ?1 || '/%'",
            params![path_prefix],
        )?;
        i64::try_from(affected).map_err(|e| AppError::Internal(format!("行数转换失败: {e}")))
    }

    /// 按路径前缀获取所有未软删除文件的 ID（清理向量索引用）。
    ///
    /// 匹配规则同 [`Self::soft_delete_by_path_prefix`]。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn get_ids_by_path_prefix(conn: &Connection, path_prefix: &str) -> AppResult<Vec<String>> {
        let mut stmt =
            conn.prepare("SELECT id FROM files WHERE is_deleted = 0 AND path LIKE ?1 || '/%'")?;
        let ids = stmt
            .query_map(params![path_prefix], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// 按路径批量回查既存分类（扫描后回显「已整理」状态用）。
    ///
    /// 返回 `path → category`，仅包含库中存在且未软删除、且已有分类的路径。
    /// `scan_directory` 扫描到的文件 category 恒为 `None`，但同一路径此前若被整理过，
    /// DB 里已存有分类；这里按 path 回查后覆盖回返回结果，前端才能正确标记。
    ///
    /// 路径数超过单条 SQL 变量上限时按 [`CHUNK_IN_PATHS`] 分块（T10.1）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn get_categories_by_paths(
        conn: &Connection,
        paths: &[String],
    ) -> AppResult<HashMap<String, String>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let mut categories = HashMap::new();
        for chunk in paths.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT path, category FROM files
                 WHERE path IN ({placeholders}) AND is_deleted = 0 AND category IS NOT NULL"
            );

            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;

            for row in rows {
                let (path, category) = row?;
                categories.insert(path, category);
            }
        }
        Ok(categories)
    }

    /// 批量加载增量扫描快照（T10.1）。
    ///
    /// 返回 `path → FileSnapshot`，**包含软删除行**（与 `get_categories_by_paths` 不同，
    /// 软删记录需复活，不能过滤）。路径数超过单条 SQL 变量上限时按 [`CHUNK_IN_PATHS`]
    /// 分块，替代旧的逐条 `SELECT ... WHERE path=?1`（`upsert_batch` 曾每文件一条查询）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn load_snapshots_by_paths(
        conn: &Connection,
        paths: &[String],
    ) -> AppResult<HashMap<String, FileSnapshot>> {
        let mut snapshots = HashMap::with_capacity(paths.len());
        for chunk in paths.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT path, id, is_deleted, content_hash, file_size, mtime
                 FROM files WHERE path IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileSnapshot {
                        id: row.get(1)?,
                        is_deleted: row.get::<_, i64>(2)? != 0,
                        content_hash: row.get(3)?,
                        file_size: row.get(4)?,
                        mtime: row.get(5)?,
                    },
                ))
            })?;
            for row in rows {
                let (path, snapshot) = row?;
                snapshots.insert(path, snapshot);
            }
        }
        Ok(snapshots)
    }

    /// 按内容哈希查找所有未删除文件（用于重复文件分组）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn get_by_hash(conn: &Connection, content_hash: &str) -> AppResult<Vec<FileRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files WHERE content_hash = ?1 AND is_deleted = 0"
        )?;

        let rows = stmt.query_map(params![content_hash], map_file_record)?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    /// 列出待向量化文件（增量索引的候选集）。
    ///
    /// 未删除且满足任一条件即入选：
    ///   - `embedding_model IS NULL`（从未建过索引）
    ///   - `embedding_model != 当前模型`（Embedding 模型已切换）
    ///   - `embedding_hash IS NULL`（建索引时未记录内容哈希）
    ///   - `embedding_hash != content_hash`（内容在索引后发生过变更）
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn list_pending_embedding(
        conn: &Connection,
        embedding_model: &str,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at, mtime
             FROM files
             WHERE is_deleted = 0
               AND (embedding_model IS NULL
                    OR embedding_model <> ?1
                    OR embedding_hash IS NULL
                    OR embedding_hash <> content_hash)
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![embedding_model, limit], map_file_record)?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    /// 向量化成功后回写状态标记（`embedding_model` + `embedding_hash`）。
    ///
    /// `entries` 为 `(file_id, 建索引时刻的 content_hash)` 列表；全部 UPDATE 包在
    /// 单事务中执行。调用方只应传入「实际写入向量的文件」，读取失败 / 非文本
    /// 文件保持未标记，下次索引自动重试。
    ///
    /// # Errors
    ///
    /// 事务开启或任一更新失败时返回数据库错误。
    pub fn mark_embedded(
        conn: &Connection,
        embedding_model: &str,
        entries: &[(String, String)],
    ) -> AppResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let tx = conn.unchecked_transaction()?;
        let mut count = 0;
        for (file_id, content_hash) in entries {
            let affected = tx.execute(
                "UPDATE files
                 SET embedding_model = ?1,
                     embedding_hash = ?2,
                     updated_at = datetime('now')
                 WHERE id = ?3 AND is_deleted = 0",
                params![embedding_model, content_hash, file_id],
            )?;
            count += affected;
        }
        tx.commit()?;
        Ok(count)
    }

    /// 清除指定文件的索引状态标记（向量已从 `LanceDB` 删除时调用）。
    ///
    /// 把 `embedding_model` / `embedding_hash` 置回 NULL，避免「向量已删但标记仍在」
    /// 导致增量索引把该文件误判为已建而跳过。ID 列表按 [`CHUNK_IN_PATHS`] 分块，
    /// 用单条 `UPDATE ... IN (...)` 批量执行。
    ///
    /// # Errors
    ///
    /// 任一 UPDATE 失败时返回数据库错误。
    pub fn clear_embedding_marker(conn: &Connection, ids: &[String]) -> AppResult<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut cleared = 0;
        for chunk in ids.chunks(CHUNK_IN_PATHS) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "UPDATE files
                 SET embedding_model = NULL,
                     embedding_hash = NULL,
                     updated_at = datetime('now')
                 WHERE id IN ({placeholders})"
            );
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            cleared += conn.execute(&sql, params.as_slice())?;
        }
        Ok(cleared)
    }
}

/// 批量增量写入统计。
#[derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)]
pub struct UpsertResult {
    /// 新增条数。
    pub added: u32,
    /// 更新条数。
    pub updated: u32,
    /// 内容未变化跳过的条数。
    pub skipped: u32,
    /// 传入 id → 入库真实 id 映射（`scan_directory` 回写用；避免同一路径重复扫描时
    /// 扫描生成的新 uuid 与库内旧 id 不一致导致按 id 反查落空）。
    #[serde(skip)]
    #[specta(skip)]
    pub id_map: HashMap<String, String>,
}

/// 将查询行映射为 [`FileRecord`]。
fn map_file_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        path: row.get(1)?,
        file_name: row.get(2)?,
        file_size: row.get(3)?,
        content_hash: row.get(4)?,
        category: row.get(5)?,
        is_deleted: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        mtime: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::db::database::Database;
    use tempfile::NamedTempFile;

    fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        Ok(Database::open(tmp.path())?)
    }

    fn mk_record(
        path: &str,
        file_size: i64,
        hash: Option<&str>,
        mtime: Option<&str>,
    ) -> FileRecord {
        FileRecord {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_string(),
            file_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            file_size,
            content_hash: hash.map(str::to_string),
            category: None,
            is_deleted: false,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
            mtime: mtime.map(str::to_string),
        }
    }

    /// 超过单条 SQL 900 参数上限（T10.1 分块），验证 `upsert_batch` /
    /// `load_snapshots_by_paths` / `get_categories_by_paths` 三条分块路径。
    /// 2000 << 2^63，usize→i64 饱和转换恒安全，测试内允许 cast。
    #[allow(clippy::cast_possible_wrap)]
    #[test]
    fn test_upsert_batch_chunked() -> Result<(), Box<dyn std::error::Error>> {
        const COUNT: usize = 2000;

        let db = setup_db()?;

        let records: Vec<FileRecord> = (0..COUNT)
            .map(|i| {
                mk_record(
                    &format!("/tmp/big/{i:05}.txt"),
                    i as i64,
                    Some("h"),
                    Some("2026-01-01 00:00:00"),
                )
            })
            .collect();

        let first = FileRepo::upsert_batch(db.conn(), &records)?;
        assert_eq!(usize::try_from(first.added)?, COUNT);
        assert_eq!(first.skipped, 0);
        assert_eq!(first.id_map.len(), COUNT);

        // 快照批量加载同样分块
        let paths: Vec<String> = records.iter().map(|r| r.path.clone()).collect();
        let snapshots = FileRepo::load_snapshots_by_paths(db.conn(), &paths)?;
        assert_eq!(snapshots.len(), COUNT);

        // 重扫（内容 + size + mtime 均未变）→ 全部跳过，不写库
        let second = FileRepo::upsert_batch_with_snapshots(db.conn(), &records, &snapshots)?;
        assert_eq!(second.skipped, u32::try_from(COUNT)?);
        assert_eq!(second.updated, 0);

        // 分类回查分块：给半数路径打分类（用 UPDATE —— upsert 的 ON CONFLICT
        // DO UPDATE 刻意不写 category，避免重扫抹掉既有分类）→ 回查应精确命中半数
        for path in paths.iter().step_by(2) {
            db.conn().execute(
                "UPDATE files SET category = '财务' WHERE path = ?1",
                rusqlite::params![path],
            )?;
        }
        let categories = FileRepo::get_categories_by_paths(db.conn(), &paths)?;
        assert_eq!(categories.len(), COUNT / 2);
        assert_eq!(
            categories.get("/tmp/big/00000.txt").map(String::as_str),
            Some("财务")
        );
        Ok(())
    }

    /// 增量扫描：未变化文件（hash/size/mtime 全一致）重扫 → skipped，不写库。
    #[test]
    fn test_incremental_skip_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let rec = mk_record(
            "/tmp/incr/a.txt",
            10,
            Some("deadbeef"),
            Some("2026-01-01 00:00:00"),
        );

        let first = FileRepo::upsert_batch(db.conn(), std::slice::from_ref(&rec))?;
        assert_eq!(first.added, 1);

        let snapshots =
            FileRepo::load_snapshots_by_paths(db.conn(), std::slice::from_ref(&rec.path))?;
        let second = FileRepo::upsert_batch_with_snapshots(db.conn(), &[rec], &snapshots)?;
        assert_eq!(second.skipped, 1);
        assert_eq!(second.updated, 0);
        Ok(())
    }

    /// 增量索引候选集：已建且模型/内容均未变 → 排除；未建 / 换模型 / 内容变更 → 入选。
    #[test]
    fn test_list_pending_embedding_filters_marked() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;

        let rec_a = mk_record("/tmp/p/a.txt", 1, Some("aa"), Some("2026-01-01 00:00:00"));
        let rec_b = mk_record("/tmp/p/b.txt", 1, Some("bb"), Some("2026-01-01 00:00:00"));
        let rec_c = mk_record("/tmp/p/c.txt", 1, Some("cc"), Some("2026-01-01 00:00:00"));
        let res = FileRepo::upsert_batch(db.conn(), &[rec_a, rec_b, rec_c])?;
        assert_eq!(res.added, 3);
        // 直接按 path 回查入库 id（新插入时与传入 uuid 一致）
        let id_of = |path: &str| -> Result<String, Box<dyn std::error::Error>> {
            Ok(db
                .conn()
                .query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0))?)
        };
        let id_a = id_of("/tmp/p/a.txt")?;
        let id_b = id_of("/tmp/p/b.txt")?;
        let id_c = id_of("/tmp/p/c.txt")?;

        // 全部未标记 → 3 个都是候选
        let all = FileRepo::list_pending_embedding(db.conn(), "m1", 100)?;
        assert_eq!(all.len(), 3);

        // 全部标记为 m1（hash 与 content_hash 一致）→ 无候选；换模型 m2 → 全量候选
        let entries: Vec<(String, String)> = vec![
            (id_a.clone(), "aa".to_string()),
            (id_b.clone(), "bb".to_string()),
            (id_c.clone(), "cc".to_string()),
        ];
        assert_eq!(FileRepo::mark_embedded(db.conn(), "m1", &entries)?, 3);
        assert!(FileRepo::list_pending_embedding(db.conn(), "m1", 100)?.is_empty());
        assert_eq!(
            FileRepo::list_pending_embedding(db.conn(), "m2", 100)?.len(),
            3
        );

        // c 内容变更（content_hash 更新为 cc2）→ 仅 c 重新入选
        db.conn().execute(
            "UPDATE files SET content_hash = 'cc2' WHERE id = ?1",
            rusqlite::params![id_c],
        )?;
        let pending = FileRepo::list_pending_embedding(db.conn(), "m1", 100)?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id_c);

        // 清除标记 → b、c 重新进入候选集；a（仍标记且内容未变）保持排除
        assert_eq!(
            FileRepo::clear_embedding_marker(db.conn(), &[id_b.clone(), id_c.clone()])?,
            2
        );
        let pending = FileRepo::list_pending_embedding(db.conn(), "m1", 100)?;
        let pending_ids: Vec<&str> = pending.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(pending_ids.len(), 2);
        assert!(pending_ids.contains(&id_b.as_str()));
        assert!(pending_ids.contains(&id_c.as_str()));
        assert!(!pending_ids.contains(&id_a.as_str()));
        Ok(())
    }

    /// 软删除文件不进入待建候选集。
    #[test]
    fn test_list_pending_embedding_excludes_deleted() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let rec = mk_record("/tmp/p/del.txt", 1, Some("dd"), Some("2026-01-01 00:00:00"));
        let res = FileRepo::upsert_batch(db.conn(), std::slice::from_ref(&rec))?;
        assert_eq!(res.added, 1);
        let id: String = db.conn().query_row(
            "SELECT id FROM files WHERE path = ?1",
            ["/tmp/p/del.txt"],
            |r| r.get(0),
        )?;

        // 软删除后即便从未标记，也不该出现在候选里
        db.conn().execute(
            "UPDATE files SET is_deleted = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        assert!(FileRepo::list_pending_embedding(db.conn(), "m1", 100)?.is_empty());
        Ok(())
    }

    /// mtime 变化但内容相同（touch 场景）→ 重新落库刷新 mtime，供下次扫描跳过。
    #[test]
    fn test_mtime_mismatch_rehash() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let rec1 = mk_record(
            "/tmp/incr/b.txt",
            10,
            Some("deadbeef"),
            Some("2026-01-01 00:00:00"),
        );
        FileRepo::upsert_batch(db.conn(), &[rec1])?;

        let rec2 = mk_record(
            "/tmp/incr/b.txt",
            10,
            Some("deadbeef"),
            Some("2026-01-05 00:00:00"),
        );
        let snapshots =
            FileRepo::load_snapshots_by_paths(db.conn(), std::slice::from_ref(&rec2.path))?;
        let result = FileRepo::upsert_batch_with_snapshots(db.conn(), &[rec2], &snapshots)?;
        assert_eq!(result.updated, 1);

        let stored: Option<String> = db.conn().query_row(
            "SELECT mtime FROM files WHERE path = '/tmp/incr/b.txt'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stored.as_deref(), Some("2026-01-05 00:00:00"));
        Ok(())
    }

    /// 非扫描调用方 `mtime` 为 `None`（分类/恢复路径）→ 退化为仅 hash 比较，
    /// 保持既有跳过语义：size 变化但 hash 相同仍跳过。
    #[test]
    fn test_mtime_none_falls_back_to_hash_only() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let mut rec = mk_record("/tmp/incr/c.txt", 10, Some("deadbeef"), None);
        FileRepo::upsert_batch(db.conn(), &[rec.clone()])?;

        let snapshots = FileRepo::load_snapshots_by_paths(db.conn(), &[rec.path.clone()])?;
        rec.file_size = 20;
        let result = FileRepo::upsert_batch_with_snapshots(db.conn(), &[rec], &snapshots)?;
        assert_eq!(result.skipped, 1);
        Ok(())
    }

    /// 目录级移除必须连同分类标签一起清空：否则重扫时 `ON CONFLICT(path)`
    /// 复活记录会保留旧 `category`，文件原地未动却仍显示「已分类」。
    #[test]
    fn test_soft_delete_by_path_prefix_clears_category() -> Result<(), Box<dyn std::error::Error>> {
        let db = setup_db()?;
        let rec = mk_record("/tmp/dir/a.txt", 10, Some("h"), None);
        FileRepo::upsert_batch(db.conn(), std::slice::from_ref(&rec))?;
        FileRepo::update_category(db.conn(), &rec.id, "代码")?;

        let removed = FileRepo::soft_delete_by_path_prefix(db.conn(), "/tmp/dir")?;
        assert_eq!(removed, 1);

        let (deleted, category) = db.conn().query_row(
            "SELECT is_deleted, category FROM files WHERE path = '/tmp/dir/a.txt'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
        )?;
        assert_eq!(deleted, 1, "软删标记应生效");
        assert_eq!(category, None, "目录移除应清空分类标签");

        // 重扫同目录：复活后不得带回旧标签
        let second = FileRepo::upsert_batch(db.conn(), &[rec])?;
        assert_eq!(second.updated, 1);
        let revived =
            FileRepo::get_by_path(db.conn(), "/tmp/dir/a.txt")?.ok_or("复活后应可查询")?;
        assert!(!revived.is_deleted);
        assert_eq!(revived.category, None, "重扫复活不应携带旧分类标签");
        Ok(())
    }
}
