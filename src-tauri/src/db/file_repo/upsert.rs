//! 写路径（增量）：批量插入与 upsert，含「(size, mtime) 未变则复用 `content_hash`」的增量判定。

use super::{FileRepo, FileSnapshot, UpsertResult};
use crate::db::models::FileRecord;
use crate::error::AppResult;
use rusqlite::{params, Connection};
use std::collections::HashMap;

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
}
