//! `SQLite` 数据库连接管理：打开、PRAGMA 调优与迁移执行。

use std::path::Path;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

mod migrations_inner {
    //! 迁移模块封装：嵌入 `src/db/migrations` 下的 SQL 迁移脚本。

    use refinery::embed_migrations;
    embed_migrations!("src/db/migrations");
}

/// 数据库句柄：包装 `SQLite` 连接并保证迁移已执行。
pub struct Database {
    /// 底层 `SQLite` 连接。
    conn: Connection,
}

impl Database {
    /// 打开（或创建）数据库，启用 WAL 与外键，并执行增量迁移。
    ///
    /// # Errors
    ///
    /// 连接创建、PRAGMA 执行或迁移失败时返回错误。
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conn = Connection::open(db_path)?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let report = migrations_inner::migrations::runner()
            .run(&mut conn)
            .map_err(|e| {
                AppError::Database(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;

        if report.applied_migrations().is_empty() {
            log::info!("Database migrations: already up to date");
        } else {
            log::info!(
                "Database migrations: applied {} migration(s)",
                report.applied_migrations().len()
            );
        }

        Ok(Self { conn })
    }

    /// 获取底层连接引用。
    pub const fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 在单个事务中执行闭包，成功则提交，失败自动回滚。
    ///
    /// # Errors
    ///
    /// 事务开启、闭包执行或提交失败时返回错误。
    pub fn transaction<F, T>(&mut self, f: F) -> AppResult<T>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<T>,
    {
        let tx = self.conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_db() -> Result<Database, Box<dyn std::error::Error>> {
        let tmp = NamedTempFile::new()?;
        let db = Database::open(tmp.path())?;
        Ok(db)
    }

    #[test]
    fn test_database_open_and_migrate() -> Result<(), Box<dyn std::error::Error>> {
        let db = create_test_db()?;
        let mut stmt = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(std::result::Result::ok)
            .collect();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"operations_log".to_string()));
        assert!(tables.contains(&"categories".to_string()));
        assert!(tables.contains(&"rules".to_string()));
        assert!(tables.contains(&"file_fts".to_string()));
        Ok(())
    }

    #[test]
    fn test_wal_mode_enabled() -> Result<(), Box<dyn std::error::Error>> {
        let db = create_test_db()?;
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(mode, "wal");
        Ok(())
    }

    #[test]
    fn test_foreign_keys_enabled() -> Result<(), Box<dyn std::error::Error>> {
        let db = create_test_db()?;
        let fk: i64 = db
            .conn()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        assert_eq!(fk, 1);
        Ok(())
    }
}
