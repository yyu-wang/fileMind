//! `file_search` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `db/file_search/mod.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。
//! 注意 `super` 指 `db::file_search` 模块，不是文件所在目录 `db`；被拆到
//! 兄弟模块的私有项（查询构造、读取上限）需显式按模块路径引入。

use super::populate::MAX_FTS_FILE_BYTES;
use super::query::{build_fts_query, sanitize_query};
use super::*;
use crate::db::database::Database;
use crate::db::file_repo::FileRepo;
use std::path::Path;
use tempfile::NamedTempFile;

fn setup_test_db_with_data() -> Result<Database, Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    let db = Database::open(tmp.path())?;

    let files = vec![
        FileRecord {
            id: "f001".to_string(),
            path: "/docs/report.pdf".to_string(),
            file_name: "report.pdf".to_string(),
            file_size: 1024,
            content_hash: Some("hash1".to_string()),
            category: Some("文档".to_string()),
            is_deleted: false,
            created_at: "2026-01-01".to_string(),
            updated_at: "2026-01-01".to_string(),
            mtime: None,
        },
        FileRecord {
            id: "f002".to_string(),
            path: "/docs/notes.md".to_string(),
            file_name: "notes.md".to_string(),
            file_size: 512,
            content_hash: Some("hash2".to_string()),
            category: Some("文档".to_string()),
            is_deleted: false,
            created_at: "2026-01-02".to_string(),
            updated_at: "2026-01-02".to_string(),
            mtime: None,
        },
    ];

    FileRepo::insert_batch(db.conn(), &files)?;
    Ok(db)
}

#[test]
fn test_search_by_filename() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_test_db_with_data()?;
    let results = FileSearch::search_by_filename(db.conn(), "report", 10)?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "report.pdf");
    Ok(())
}

#[test]
fn test_search_by_filename_no_match() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_test_db_with_data()?;
    let results = FileSearch::search_by_filename(db.conn(), "nonexistent", 10)?;
    assert!(results.is_empty());
    Ok(())
}

#[test]
fn test_search_empty_query() -> Result<(), Box<dyn std::error::Error>> {
    let db = setup_test_db_with_data()?;
    let results = FileSearch::search(db.conn(), "", 10)?;
    assert!(results.is_empty());
    Ok(())
}

#[test]
fn test_sanitize_query() {
    // 英文：保留字母数字与空格
    assert_eq!(sanitize_query("hello world"), "hello world");
    // 注入防御：FTS5 特殊字符 ``;`` 被过滤
    assert_eq!(sanitize_query("hello; DROP TABLE"), "hello DROP TABLE");
    assert_eq!(sanitize_query(""), "");
    // 中文：is_alphanumeric 对 CJK 返回 true，应原样保留
    assert_eq!(sanitize_query("文件管理"), "文件管理");
    assert_eq!(sanitize_query("FileMind 文件管理"), "FileMind 文件管理");
    // 标点过滤：`,` `!` 等被移除，空格保留
    assert_eq!(sanitize_query("hello, world!"), "hello world");
    // 下划线保留：常见于文件名
    assert_eq!(sanitize_query("my_report"), "my_report");
    // FTS5 通配符与引号被过滤，防止语法注入
    assert_eq!(sanitize_query("hello*\" OR 1=1"), "hello OR 11");
}

#[test]
fn test_build_fts_query() {
    // 空字符串
    assert_eq!(build_fts_query(""), "");
    // 纯 ASCII 查询：保持整句前缀匹配
    assert_eq!(build_fts_query("hello"), "\"hello\"*");
    assert_eq!(build_fts_query("hello world"), "\"hello world\"*");
    // 含 CJK 字符：取前 2 字作前缀
    assert_eq!(build_fts_query("分类"), "\"分类\"*");
    assert_eq!(build_fts_query("文件管理"), "\"文件\"*");
    assert_eq!(build_fts_query("分类整理的流程是什么"), "\"分类\"*");
}

/// 构造指向真实临时文件的 `FileRecord`。
fn record_for(id: &str, path: &Path, file_name: &str, is_deleted: bool) -> FileRecord {
    FileRecord {
        id: id.to_string(),
        path: path.to_string_lossy().to_string(),
        file_name: file_name.to_string(),
        file_size: 100,
        content_hash: None,
        category: None,
        is_deleted,
        created_at: "2026-01-01".to_string(),
        updated_at: "2026-01-01".to_string(),
        mtime: None,
    }
}

#[test]
fn test_populate_fts_content_index_and_skip_branches() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    let db = Database::open(tmp.path())?;
    let dir = tempfile::tempdir()?;

    // 1) 正常文本文件 → indexed
    let md_path = dir.path().join("a.md");
    std::fs::write(&md_path, "FileMind 文件管理 整理分类流程")?;
    // 2) 非文本扩展名 → skipped
    let bin_path = dir.path().join("b.exe");
    std::fs::write(&bin_path, "binary")?;
    // 3) 路径不存在（is_file=false）→ skipped
    let ghost_path = dir.path().join("ghost.md");
    // 4) 空白内容 → skipped
    let empty_path = dir.path().join("empty.txt");
    std::fs::write(&empty_path, "   ")?;
    // 5) 超过 50MB 上限 → skipped
    let big_path = dir.path().join("big.txt");
    let big_len = usize::try_from(MAX_FTS_FILE_BYTES + 1)?;
    std::fs::write(&big_path, vec![b'x'; big_len])?;

    let files = vec![
        record_for("f100", &md_path, "a.md", false),
        record_for("f101", &bin_path, "b.exe", false),
        record_for("f102", &ghost_path, "ghost.md", false),
        record_for("f103", &empty_path, "empty.txt", false),
        record_for("f104", &big_path, "big.txt", false),
    ];
    FileRepo::insert_batch(db.conn(), &files)?;

    let (indexed, skipped) = FileSearch::populate_fts_content(db.conn(), &files)?;
    assert_eq!(indexed, 1);
    assert_eq!(skipped, 4);

    // FTS 全文搜索命中已入库正文（CJK 前 2 字前缀匹配）
    let results = FileSearch::search(db.conn(), "整理", 10)?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file.file_name, "a.md");
    assert_eq!(results[0].file.id, "f100");
    Ok(())
}

#[test]
fn test_populate_fts_content_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    let db = Database::open(tmp.path())?;
    let dir = tempfile::tempdir()?;

    let md_path = dir.path().join("a.md");
    std::fs::write(&md_path, "知识库检索测试")?;
    let file = record_for("f200", &md_path, "a.md", false);
    FileRepo::insert_batch(db.conn(), std::slice::from_ref(&file))?;

    // 同一文件重复填充：删旧插新，不产生重复行
    FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;
    let (indexed_again, _) =
        FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;

    assert_eq!(indexed_again, 1);
    let results = FileSearch::search(db.conn(), "知识", 10)?;
    assert_eq!(results.len(), 1);
    Ok(())
}

#[test]
fn test_search_excludes_deleted_files() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    let db = Database::open(tmp.path())?;
    let dir = tempfile::tempdir()?;

    let md_path = dir.path().join("deleted.md");
    std::fs::write(&md_path, "已删除文件内容")?;
    let file = record_for("f300", &md_path, "deleted.md", true);
    FileRepo::insert_batch(db.conn(), std::slice::from_ref(&file))?;

    let (indexed, _) = FileSearch::populate_fts_content(db.conn(), std::slice::from_ref(&file))?;
    assert_eq!(indexed, 1); // FTS 填充不看软删除标记

    // 全文搜索过滤 is_deleted=1
    let results = FileSearch::search(db.conn(), "删除", 10)?;
    assert!(results.is_empty());
    Ok(())
}
