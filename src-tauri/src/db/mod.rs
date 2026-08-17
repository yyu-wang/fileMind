//! 数据访问层：数据库连接、文件仓库与搜索。

pub mod database;
pub mod file_repo;
pub mod file_search;
pub mod models;

pub use database::Database;
pub use file_repo::{FileRepo, UpsertResult};
pub use file_search::{FileSearch, SearchResult};
