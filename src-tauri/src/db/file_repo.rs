use rusqlite::{params, Connection};

use crate::db::models::FileRecord;
use crate::error::{AppError, AppResult};

const INSERT_FILE_SQL: &str = "
    INSERT INTO files (id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, datetime('now'), datetime('now'))
    ON CONFLICT(path) DO UPDATE SET
        file_name = excluded.file_name,
        file_size = excluded.file_size,
        content_hash = excluded.content_hash,
        updated_at = datetime('now')
";

const GET_BY_PATH_SQL: &str = "
    SELECT id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at
    FROM files WHERE path = ?1 AND is_deleted = 0
";

pub struct FileRepo;

impl FileRepo {
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

    pub fn get_by_path(conn: &Connection, path: &str) -> AppResult<Option<FileRecord>> {
        let mut stmt = conn.prepare(GET_BY_PATH_SQL)?;
        let result = stmt.query_row(params![path], map_file_record);

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

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

    pub fn list(
        conn: &Connection,
        category: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<FileRecord>> {
        let sql = match category {
            Some(cat) => {
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

    pub fn update_category(
        conn: &Connection,
        id: &str,
        category: &str,
    ) -> AppResult<()> {
        let affected = conn.execute(
            "UPDATE files SET category = ?1, updated_at = datetime('now') WHERE id = ?2 AND is_deleted = 0",
            params![category, id],
        )?;

        if affected == 0 {
            return Err(AppError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }

    pub fn upsert_batch(
        conn: &Connection,
        files: &[FileRecord],
    ) -> AppResult<UpsertResult> {
        let tx = conn.unchecked_transaction()?;
        let mut result = UpsertResult::default();

        for file in files {
            let existing = tx.query_row(
                "SELECT content_hash FROM files WHERE path = ?1 AND is_deleted = 0",
                params![file.path],
                |row| row.get::<_, Option<String>>(0),
            );

            match existing {
                Ok(Some(hash)) if hash == file.content_hash => {
                    result.skipped += 1;
                }
                _ => {
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
                    if matches!(existing, Ok(Some(_))) {
                        result.updated += 1;
                    } else {
                        result.added += 1;
                    }
                }
            }
        }

        tx.commit()?;
        Ok(result)
    }

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

    pub fn get_by_hash(
        conn: &Connection,
        content_hash: &str,
    ) -> AppResult<Vec<FileRecord>> {
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

#[derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)]
pub struct UpsertResult {
    pub added: u32,
    pub updated: u32,
    pub skipped: u32,
}

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
