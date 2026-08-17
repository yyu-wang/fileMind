use std::path::Path;

use refinery::embed_migrations;
use rusqlite::Connection;

use crate::error::{AppError, AppResult};

embed_migrations!("src/db/migrations");

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let report = migrations::runner(&conn).run().map_err(|e| {
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

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

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

    fn create_test_db() -> Database {
        let tmp = NamedTempFile::new().unwrap();
        Database::open(tmp.path()).unwrap()
    }

    #[test]
    fn test_database_open_and_migrate() {
        let db = create_test_db();
        let tables: Vec<String> = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"operations_log".to_string()));
        assert!(tables.contains(&"categories".to_string()));
        assert!(tables.contains(&"rules".to_string()));
        assert!(tables.contains(&"file_fts".to_string()));
    }

    #[test]
    fn test_wal_mode_enabled() {
        let db = create_test_db();
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = create_test_db();
        let fk: i64 = db
            .conn()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
