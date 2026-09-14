//! `file_repo` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests/` 目录）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::db::database::Database;
use tempfile::NamedTempFile;

fn setup_db() -> Result<Database, Box<dyn std::error::Error>> {
    let tmp = NamedTempFile::new()?;
    Ok(Database::open(tmp.path())?)
}

fn mk_record(path: &str, file_size: i64, hash: Option<&str>, mtime: Option<&str>) -> FileRecord {
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

    let snapshots = FileRepo::load_snapshots_by_paths(db.conn(), std::slice::from_ref(&rec.path))?;
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
    let snapshots = FileRepo::load_snapshots_by_paths(db.conn(), std::slice::from_ref(&rec2.path))?;
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
    let revived = FileRepo::get_by_path(db.conn(), "/tmp/dir/a.txt")?.ok_or("复活后应可查询")?;
    assert!(!revived.is_deleted);
    assert_eq!(revived.category, None, "重扫复活不应携带旧分类标签");
    Ok(())
}
