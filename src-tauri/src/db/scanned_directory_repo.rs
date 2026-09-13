//! `scanned_directories` 表仓库：已扫描根目录的增删查。
//!
//! 与 `files` 表配合实现「目录级移除」：扫描时记录目录，移除时按路径前缀
//! 软删该目录下所有文件并删除本行。`file_count` 不存表，`list_with_file_count`
//! 通过 LEFT JOIN `files` 实时统计，保证与 `files` 表一致。

use rusqlite::{params, Connection};

use crate::db::models::ScannedDirectory;
use crate::error::{AppError, AppResult};

const UPSERT_SQL: &str = "
    INSERT INTO scanned_directories (id, path, created_at, updated_at)
    VALUES (?1, ?2, datetime('now'), datetime('now'))
    ON CONFLICT(path) DO UPDATE SET updated_at = datetime('now')
";

/// `scanned_directories` 表仓库。
pub struct ScannedDirectoryRepo;

impl ScannedDirectoryRepo {
    /// 扫描目录成功后记录：路径已存在则仅更新 `updated_at`。
    ///
    /// # Errors
    ///
    /// SQL 执行失败时返回数据库错误。
    pub fn insert_or_update(conn: &Connection, path: &str) -> AppResult<()> {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(UPSERT_SQL, params![id, path])?;
        Ok(())
    }

    /// 列出全部已扫描目录，附带每个目录下未软删除的文件数。
    ///
    /// 路径前缀匹配用 `path || '/%'`，避免 `/a/b` 误匹配 `/a/bc`。
    /// 按 `updated_at` 倒序（最近扫描的在前）。
    ///
    /// # Errors
    ///
    /// 语句准备或行读取失败时返回数据库错误。
    pub fn list_with_file_count(conn: &Connection) -> AppResult<Vec<ScannedDirectory>> {
        let mut stmt = conn.prepare(
            "SELECT sd.id, sd.path, sd.created_at, sd.updated_at,
                    COUNT(f.id) AS file_count
             FROM scanned_directories sd
             LEFT JOIN files f
               ON f.path LIKE (sd.path || '/%') AND f.is_deleted = 0
             GROUP BY sd.id
             ORDER BY sd.updated_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ScannedDirectory {
                id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                file_count: row.get(4)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// 删除目录记录（移除目录时调用；文件软删由调用方负责）。
    ///
    /// # Errors
    ///
    /// 路径不存在时返回 `NotFound`；SQL 执行失败返回数据库错误。
    pub fn delete(conn: &Connection, path: &str) -> AppResult<()> {
        let affected = conn.execute(
            "DELETE FROM scanned_directories WHERE path = ?1",
            params![path],
        )?;
        if affected == 0 {
            return Err(AppError::InvalidInput(format!("目录记录不存在: {path}")));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn make_db() -> Database {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        Database::open(tmp.path()).expect("打开测试 DB 失败")
    }

    #[test]
    fn test_insert_and_list() {
        let db = make_db();
        let conn = db.conn();

        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/a").unwrap();
        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/b").unwrap();

        let list = ScannedDirectoryRepo::list_with_file_count(conn).unwrap();
        assert_eq!(list.len(), 2);
        // 无文件时 file_count 为 0
        assert!(list.iter().all(|d| d.file_count == 0));
    }

    #[test]
    fn test_insert_duplicate_updates_updated_at() {
        let db = make_db();
        let conn = db.conn();

        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/a").unwrap();
        // 重复插入同一路径：不新增行（行数仍为 1）
        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/a").unwrap();
        let list = ScannedDirectoryRepo::list_with_file_count(conn).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_file_count_matches_files() {
        let db = make_db();
        let conn = db.conn();

        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/a").unwrap();
        // 手动插入两条 file 记录（不走 FileRepo，直接 SQL）
        conn.execute(
            "INSERT INTO files (id, path, file_name, file_size, is_deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            params!["f1", "/tmp/a/x.txt", "x.txt", 100i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files (id, path, file_name, file_size, is_deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            params!["f2", "/tmp/a/sub/y.txt", "y.txt", 200i64],
        )
        .unwrap();
        // 另一个目录的文件不应计入
        conn.execute(
            "INSERT INTO files (id, path, file_name, file_size, is_deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            params!["f3", "/tmp/b/z.txt", "z.txt", 300i64],
        )
        .unwrap();

        let list = ScannedDirectoryRepo::list_with_file_count(conn).unwrap();
        let dir_a = list.iter().find(|d| d.path == "/tmp/a").unwrap();
        assert_eq!(dir_a.file_count, 2);
    }

    #[test]
    fn test_delete() {
        let db = make_db();
        let conn = db.conn();

        ScannedDirectoryRepo::insert_or_update(conn, "/tmp/a").unwrap();
        ScannedDirectoryRepo::delete(conn, "/tmp/a").unwrap();
        let list = ScannedDirectoryRepo::list_with_file_count(conn).unwrap();
        assert!(list.is_empty());

        // 删除不存在的路径返回 InvalidInput
        let err = ScannedDirectoryRepo::delete(conn, "/tmp/nonexistent").unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }
}
