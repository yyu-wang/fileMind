//! 数据访问层：数据库连接、文件/分类/规则/操作日志仓库与全文搜索。

pub mod category_repo;
pub mod config_repo;
pub mod database;
pub mod file_repo;
pub mod file_search;
pub mod models;
pub mod operation_repo;
pub mod rule_repo;
pub mod scanned_directory_repo;

pub use category_repo::CategoryRepo;
pub use config_repo::ConfigRepo;
pub use database::Database;
pub use file_repo::{FileRepo, UpsertResult};
pub use file_search::{FileSearch, SearchResult};
pub use models::{CategoryNode, OperationBatchSummary, OperationLog, ScannedDirectory};
pub use operation_repo::OperationRepo;
pub use rule_repo::RuleRepo;
pub use scanned_directory_repo::ScannedDirectoryRepo;
