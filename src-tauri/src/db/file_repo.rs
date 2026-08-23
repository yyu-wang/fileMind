//! `files` 表数据仓库：批量写入、查询、分类更新与软删除。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};

const INSERT_FILE_SQL: &str = "
    INSERT INTO files (id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, datetime('now'), datetime('now'))
    ON CONFLICT(path) DO UPDATE SET
        file_name = excluded.file_name,
        file_size = excluded.file_size,
        content_hash = excluded.content_hash,
        is_deleted = 0,
        updated_at = datetime('now')
";

const GET_BY_PATH_SQL: &str = "
    SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
    FROM files WHERE path = ?1 AND is_deleted = 0
";

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
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
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
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
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
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
                 FROM files WHERE is_deleted = 0 AND category = ?1
                 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"
            }
            None => {
                "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
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
    /// # Errors
    ///
    /// 事务开启、查询、写入或提交失败时返回错误。
    pub fn upsert_batch(conn: &Connection, files: &[FileRecord]) -> AppResult<UpsertResult> {
        let tx = conn.unchecked_transaction()?;
        let mut result = UpsertResult::default();

        for file in files {
            // 按 path 反查已存在记录（含软删除行）：
            // INSERT_FILE_SQL 的 ON CONFLICT(path) 只会更新元数据、保留旧 id，
            // 若扫描生成的 id 不等于旧 id，后续按 id 反查（classify/preview/execute）会落空，
            // 必须把「传入 id → 入库 id」记入 id_map 供 scan_directory 回写。
            let existing = tx.query_row(
                "SELECT id, is_deleted, content_hash FROM files WHERE path = ?1",
                params![file.path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            );

            match existing {
                Ok((existing_id, is_deleted, existing_hash)) => {
                    result.id_map.insert(file.id.clone(), existing_id);
                    if is_deleted == 0 && file.content_hash.as_ref() == existing_hash.as_ref() {
                        result.skipped += 1;
                    } else {
                        // 路径已存在但内容/软删状态变化：复用 INSERT_FILE_SQL 的
                        // ON CONFLICT(path) DO UPDATE 刷新元数据并复活记录（保留旧 id）
                        tx.execute(
                            INSERT_FILE_SQL,
                            params![
                                file.id,
                                file.path,
                                file.file_name,
                                file.file_size,
                                file.content_hash,
                                file.category,
                            ],
                        )?;
                        result.updated += 1;
                    }
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    tx.execute(
                        INSERT_FILE_SQL,
                        params![
                            file.id,
                            file.path,
                            file.file_name,
                            file.file_size,
                            file.content_hash,
                            file.category,
                        ],
                    )?;
                    result.id_map.insert(file.id.clone(), file.id.clone());
                    result.added += 1;
                }
                Err(e) => return Err(e.into()),
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
            "UPDATE files SET is_deleted = 1, updated_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    /// 按路径批量回查既存分类（扫描后回显「已整理」状态用）。
    ///
    /// 返回 `path → category`，仅包含库中存在且未软删除、且已有分类的路径。
    /// `scan_directory` 扫描到的文件 category 恒为 `None`，但同一路径此前若被整理过，
    /// DB 里已存有分类；这里按 path 回查后覆盖回返回结果，前端才能正确标记。
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

        let placeholders = paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT path, category FROM files
             WHERE path IN ({placeholders}) AND is_deleted = 0 AND category IS NOT NULL"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut categories = HashMap::new();
        for row in rows {
            let (path, category) = row?;
            categories.insert(path, category);
        }
        Ok(categories)
    }

    /// 按内容哈希查找所有未删除文件（用于重复文件分组）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回错误。
    pub fn get_by_hash(conn: &Connection, content_hash: &str) -> AppResult<Vec<FileRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
             FROM files WHERE content_hash = ?1 AND is_deleted = 0"
        )?;

        let rows = stmt.query_map(params![content_hash], map_file_record)?;
        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
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
    })
}
