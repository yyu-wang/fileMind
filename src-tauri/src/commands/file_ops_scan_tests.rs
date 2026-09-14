//! 扫描相关测试：目录遍历、黑名单/隐藏目录跳过、哈希计算、落库与重扫（IT-001 单元层）。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::file_ops_test_support::{create_temp_file, make_test_app_state};
use super::*;

#[test]
fn test_scan_files_finds_files_with_hash() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    create_temp_file(tmp.path(), "a.txt", "hello")?;
    create_temp_file(tmp.path(), "b.md", "world")?;

    // T10.1 重构后 collection 阶段不含 hash；hash 在持久化前并行计算，
    // 这里用同一路径（scan → compute_hashes_parallel）验证 hash 正确性
    let mut files = scan_files_on_disk(tmp.path())?;
    assert_eq!(files.len(), 2);
    let needs_hash: Vec<bool> = files.iter().map(|_| true).collect();
    compute_hashes_parallel(&mut files, &needs_hash);
    for f in &files {
        let hash = f.content_hash.as_deref().ok_or("hash 未计算")?;
        assert_eq!(hash.len(), 64, "hash 长度应为 64 hex: {hash}");
        assert!(
            hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "hash 非十六进制小写: {hash}"
        );
    }
    Ok(())
}

#[test]
fn test_scan_files_recursive_hash() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let subdir = tmp.path().join("subdir");
    std::fs::create_dir(&subdir)?;
    create_temp_file(tmp.path(), "top.txt", "top")?;
    create_temp_file(&subdir, "nested.txt", "nested")?;

    let mut files = scan_files_on_disk(tmp.path())?;
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|f| f.file_name == "nested.txt"));
    // 两个不同内容 hash 不同
    let needs_hash: Vec<bool> = files.iter().map(|_| true).collect();
    compute_hashes_parallel(&mut files, &needs_hash);
    let mut hashes: Vec<String> = files
        .iter()
        .map(|f| f.content_hash.clone().expect("hash not none"))
        .collect();
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 2, "两个不同文件 hash 应该不同");
    Ok(())
}

#[test]
fn test_scan_files_respects_max_depth() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let mut current = tmp.path().to_path_buf();
    for i in 0..(MAX_SCAN_DEPTH + 2) {
        current = current.join(format!("d{i}"));
        std::fs::create_dir(&current)?;
    }
    create_temp_file(&current, "deep.txt", "deep")?;

    let files = scan_files_on_disk(tmp.path())?;
    assert!(files.iter().all(|f| f.file_name != "deep.txt"));
    Ok(())
}

#[test]
fn test_scan_skips_blacklist_dirs() -> Result<(), Box<dyn std::error::Error>> {
    // 黑名单目录：
    // - node_modules / dist / target / __pycache__：显式列表跳过
    // - .git / .venv：以 `.` 开头的隐藏目录，由 starts_with('.') 规则跳过
    let tmp = tempfile::tempdir()?;
    for dir in [
        "node_modules",
        ".git",
        "dist",
        "target",
        "__pycache__",
        ".venv",
    ] {
        std::fs::create_dir_all(tmp.path().join(dir))?;
        create_temp_file(&tmp.path().join(dir), "ignored.js", "x")?;
    }
    // 嵌套黑名单：src/node_modules/ 也应跳过
    std::fs::create_dir_all(tmp.path().join("src/node_modules/pkg"))?;
    create_temp_file(&tmp.path().join("src/node_modules/pkg"), "deep.js", "x")?;
    // 正常文件应保留
    create_temp_file(tmp.path(), "keep.txt", "keep")?;
    create_temp_file(&tmp.path().join("src"), "keep2.txt", "keep")?;

    let files = scan_files_on_disk(tmp.path())?;
    let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
    assert!(names.contains(&"keep.txt"));
    assert!(names.contains(&"keep2.txt"));
    assert!(
        !names.contains(&"ignored.js"),
        "黑名单目录内文件不应被扫描: {names:?}"
    );
    assert!(!names.contains(&"deep.js"), "嵌套黑名单目录应跳过");
    Ok(())
}

#[test]
fn test_scan_skips_hidden_dirs() -> Result<(), Box<dyn std::error::Error>> {
    // 所有以 `.` 开头的目录应统一跳过（系统/应用数据，非用户文件）
    let tmp = tempfile::tempdir()?;

    // 隐藏目录及其文件
    for dir in [".git", ".venv"] {
        std::fs::create_dir_all(tmp.path().join(dir))?;
        create_temp_file(&tmp.path().join(dir), "data.bin", "x")?;
    }
    // 嵌套隐藏目录：在 .git 下创建子目录和文件
    std::fs::create_dir_all(tmp.path().join(".git/objects"))?;
    create_temp_file(&tmp.path().join(".git/objects"), "nested.txt", "x")?;

    // 正常文件应保留
    create_temp_file(tmp.path(), "report.pdf", "pdf")?;
    std::fs::create_dir_all(tmp.path().join("Documents"))?;
    create_temp_file(&tmp.path().join("Documents"), "doc.txt", "doc")?;

    let files = scan_files_on_disk(tmp.path())?;
    let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
    assert!(names.contains(&"report.pdf"), "正常文件应保留: {names:?}");
    assert!(
        names.contains(&"doc.txt"),
        "正常子目录文件应保留: {names:?}"
    );
    assert!(
        !names.contains(&"data.bin"),
        "隐藏目录内文件不应被扫描: {names:?}"
    );
    assert!(
        !names.contains(&"nested.txt"),
        "嵌套隐藏目录内文件不应被扫描: {names:?}"
    );
    Ok(())
}

#[test]
fn test_scan_skips_blacklist_case_insensitive() -> Result<(), Box<dyn std::error::Error>> {
    // 目录名大小写不敏感：Node_Modules 也应跳过
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join("Node_Modules"))?;
    create_temp_file(&tmp.path().join("Node_Modules"), "a.js", "x")?;
    create_temp_file(tmp.path(), "keep.txt", "keep")?;

    let files = scan_files_on_disk(tmp.path())?;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_name, "keep.txt");
    Ok(())
}

#[test]
fn test_scan_skips_garbage_files() -> Result<(), Box<dyn std::error::Error>> {
    // .DS_Store / Thumbs.db 垃圾文件跳过（大小写不敏感）
    let tmp = tempfile::tempdir()?;
    create_temp_file(tmp.path(), ".DS_Store", "")?;
    create_temp_file(tmp.path(), "Thumbs.db", "")?;
    create_temp_file(tmp.path(), "real.txt", "x")?;

    let files = scan_files_on_disk(tmp.path())?;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_name, "real.txt");
    Ok(())
}

#[test]
fn test_spawn_index_path_sync_skips_without_psk() {
    // PSK 未握手（测试环境恒为 None）时静默跳过：不 spawn、不 panic；
    // 空映射直接返回。保证 sidecar 不可用时索引同步绝不影响执行结果。
    let tmp_db = tempfile::NamedTempFile::new().unwrap();
    let state = make_test_app_state(tmp_db.path());

    spawn_index_path_sync(&state, vec![("fid1".into(), "/new/a".into())]);
    spawn_index_path_sync(&state, vec![]);
}

#[test]
fn test_format_system_time() {
    let now = std::time::SystemTime::now();
    let formatted = format_system_time(now);
    assert!(formatted.contains('-'));
    assert!(formatted.contains(':'));
}

#[test]
fn test_scan_directory_writes_sqlite() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 准备：临时扫描目录 + 3 个文件
    let scan_root = tempfile::tempdir()?;
    create_temp_file(scan_root.path(), "one.pdf", "one")?;
    create_temp_file(scan_root.path(), "two.docx", "two")?;
    create_temp_file(scan_root.path(), "three.jpg", "three")?;

    // 2. 准备：临时 SQLite DB + AppState
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 3. 执行：scan_directory 的写库路径（tauri State 不便在 test 下构造，
    //    直接调用与 scan_directory 相同的 persist_scan_files，保证写库逻辑一致；
    //    hash 由 persist_scan_files 在锁外并行计算）
    let mut files = scan_files_on_disk(scan_root.path())?;
    assert_eq!(files.len(), 3);
    persist_scan_files(&state.db, &mut files)?;

    // 4. 验证：查 files 表，存在 3 条记录且 hash 非空
    let db = state.db.lock().unwrap();
    let count: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 3, "files 表落库条目数不对");

    let null_hash_count: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE is_deleted = 0 AND content_hash IS NULL",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(null_hash_count, 0, "有未计算 hash 的文件记录落库");

    Ok(())
}

#[test]
fn test_rescan_keeps_stable_ids() -> Result<(), Box<dyn std::error::Error>> {
    // 同一目录重复扫描：返回 id 必须稳定且可被 get_by_ids 反查（修复前新 uuid 未入库）
    let scan_root = tempfile::tempdir()?;
    create_temp_file(scan_root.path(), "a.txt", "aaa")?;
    create_temp_file(scan_root.path(), "b.txt", "bbb")?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let mut first = scan_files_on_disk(scan_root.path())?;
    assert_eq!(first.len(), 2);
    persist_scan_files(&state.db, &mut first)?;

    let mut second = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut second)?;

    // 第二次扫描的临时 id 应回写为第一次入库的 id（同路径同 hash → skip）
    let first_ids: HashSet<&str> = first.iter().map(|f| f.id.as_str()).collect();
    let second_ids: HashSet<&str> = second.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(first_ids, second_ids, "同一目录重复扫描 id 应保持一致");

    // 扫描返回的 id 必须全部可被反查（修复前会落空触发"文件不存在"）
    let ids: Vec<String> = second.iter().map(|f| f.id.clone()).collect();
    let conn_guard = state.db.lock().unwrap();
    let records = FileRepo::get_by_ids(conn_guard.conn(), &ids)?;
    assert_eq!(records.len(), 2, "按扫描返回的 id 反查应全部命中");

    // 库中不应因重复扫描产生重复行
    let count: i64 = conn_guard.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 2, "重复扫描不应新增记录");
    Ok(())
}

#[test]
fn test_rescan_rehydrates_category() -> Result<(), Box<dyn std::error::Error>> {
    // 已整理文件重新扫描后：返回结果应回显 DB 中既存分类，而非恒为 None
    let scan_root = tempfile::tempdir()?;
    create_temp_file(scan_root.path(), "a.txt", "v1")?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    // 第一次扫描落库，然后模拟已整理：给文件打上分类标签
    let mut first = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut first)?;
    {
        let conn_guard = state.db.lock().unwrap();
        FileRepo::update_category(conn_guard.conn(), &first[0].id, "文档")?;
    }

    // 重新扫描：内容未变 → upsert 跳过（保留分类），persist 应把分类回显到返回值
    let mut second = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut second)?;

    assert_eq!(
        second[0].category.as_deref(),
        Some("文档"),
        "已整理文件重扫后应回显既存分类"
    );
    Ok(())
}

#[test]
fn test_rescan_unorganized_keeps_none_category() -> Result<(), Box<dyn std::error::Error>> {
    // 未整理过的文件重扫后 category 仍为 None（不回显成别的值）
    let scan_root = tempfile::tempdir()?;
    create_temp_file(scan_root.path(), "raw.txt", "x")?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let mut first = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut first)?;
    let mut second = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut second)?;

    assert!(
        second[0].category.is_none(),
        "未整理文件 category 应保持 None"
    );
    Ok(())
}

#[test]
fn test_upsert_id_map_keeps_id_on_content_change() -> Result<(), Box<dyn std::error::Error>> {
    // 文件内容变化：应 UPDATE 既有行而非新增，且 id 保持不变
    let scan_root = tempfile::tempdir()?;
    create_temp_file(scan_root.path(), "a.txt", "v1")?;
    let tmp_db = tempfile::NamedTempFile::new()?;
    let state = make_test_app_state(tmp_db.path());

    let mut first = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut first)?;
    let original_id = first[0].id.clone();

    std::fs::write(scan_root.path().join("a.txt"), b"v2")?;
    let mut second = scan_files_on_disk(scan_root.path())?;
    persist_scan_files(&state.db, &mut second)?;
    assert_eq!(second[0].id, original_id, "内容变化时仍应复用原 id");

    let conn_guard = state.db.lock().unwrap();
    let count: i64 = conn_guard.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE path = ?1 AND is_deleted = 0",
        rusqlite::params![second[0].path],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1, "内容变化应更新而非新增记录");
    Ok(())
}

#[test]
fn test_fileinfo_to_filerecord_from_impl() -> Result<(), Box<dyn std::error::Error>> {
    // From<&FileInfo> for FileRecord 手工验证：is_deleted=false、file_size 转 i64 正确
    let fi = FileInfo {
        id: "id".into(),
        path: "/a/b.txt".into(),
        file_name: "b.txt".into(),
        file_size: 4096,
        content_hash: Some("abc".into()),
        category: Some("cat".into()),
        created_at: "2025-01-01".into(),
        updated_at: "2025-01-02".into(),
    };
    let rec: FileRecord = (&fi).into();
    assert_eq!(rec.id, "id");
    assert_eq!(rec.file_size, 4096);
    assert_eq!(rec.content_hash.as_deref(), Some("abc"));
    assert_eq!(rec.category.as_deref(), Some("cat"));
    assert!(!rec.is_deleted);
    assert_eq!(
        rec.mtime.as_deref(),
        Some("2025-01-02"),
        "mtime 应取自 FileInfo.updated_at（扫描阶段仍是磁盘真实 mtime）"
    );
    Ok(())
}
