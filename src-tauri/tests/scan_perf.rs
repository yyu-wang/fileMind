//! T10.5 扫描性能基准：全量 vs 增量耗时对照（`--ignored` 门控，零依赖）。
//!
//! 度量 M1（T10.1）增量扫描的真实收益：同一目录扫两次，第二次磁盘
//! `(size, mtime)` 与快照一致 → 跳过哈希与写库，耗时应显著更低。
//!
//! 运行：
//! ```text
//! cargo test --release --test scan_perf -- --ignored
//! ```
//!
//! 规模：env `FILEMIND_BENCH_SCAN_COUNT`（默认 `10_000`；小规模冒烟设 `2_000`）。
//! 输出：JSON 写入 `FILEMIND_BENCH_OUT`（默认系统临时目录），由
//! `benchmarks/run-all.sh` 合并进 `docs/e10-bench-report.json`。
//!
//! 数据自包含：临时目录生成 `count` 个文件 + 临时 SQLite，不依赖
//! `gen_testdata.py`（避免跨语言耦合）；用户大基准仅需调大 count。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use filemind_lib::commands::file_ops::scan_and_persist;
use filemind_lib::db::Database;

const DEFAULT_COUNT: usize = 10_000;
/// 目录树层次（深度 3），模拟真实文件夹分布。
const SUBDIRS: &[&str] = &["", "sub1", "sub1/sub2"];

/// 生成 `count` 个文件的目录树（内容大小随 index 变化，接近真实分布）。
fn make_tree(root: &Path, count: usize) -> std::io::Result<()> {
    let base = b"FileMind bench payload: the quick brown fox jumps over the lazy dog 0123456789";
    for sub in SUBDIRS {
        std::fs::create_dir_all(root.join(sub))?;
    }
    for i in 0..count {
        let dir = root.join(SUBDIRS[i % SUBDIRS.len()]);
        let size = 4096 + (i % 8) * 1024;
        let mut buf = base.repeat(size / base.len() + 1);
        buf.truncate(size);
        std::fs::write(dir.join(format!("bench-{i:06}.txt")), buf)?;
    }
    Ok(())
}

/// 基准 JSON 输出路径：env `FILEMIND_BENCH_OUT`，缺省系统临时目录。
fn bench_output_path() -> PathBuf {
    std::env::var_os("FILEMIND_BENCH_OUT").map_or_else(
        || std::env::temp_dir().join("filemind-scan-perf.json"),
        PathBuf::from,
    )
}

/// 全量 vs 增量扫描耗时基准（产出 `{"count", "scan_full_ms", "scan_incremental_ms"}`）。
#[test]
#[ignore = "性能基准：需真实磁盘 IO，按需运行（见 benchmarks/README.md）"]
fn scan_perf_full_vs_incremental() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::var("FILEMIND_BENCH_SCAN_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_COUNT);

    let root = tempfile::tempdir()?;
    make_tree(root.path(), count)?;

    let db_file = tempfile::NamedTempFile::new()?;
    let db = Mutex::new(Database::open(db_file.path())?);

    // 全量（冷）：全部文件需哈希 + 入库
    let started = Instant::now();
    let files = scan_and_persist(&db, root.path())?;
    let full_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(files.len(), count, "全量扫描应返回 {count} 个文件");

    // 增量（未变化）：磁盘 (size, mtime) 与快照一致 → 跳过哈希与写库
    let started = Instant::now();
    let files2 = scan_and_persist(&db, root.path())?;
    let incr_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(files2.len(), count, "增量扫描应返回 {count} 个文件");
    assert!(
        incr_ms < full_ms,
        "增量 {incr_ms:.1}ms 应快于全量 {full_ms:.1}ms（M1 跳过哈希/写库）"
    );

    let json = format!(
        r#"{{"count":{count},"scan_full_ms":{full_ms:.1},"scan_incremental_ms":{incr_ms:.1}}}"#
    );
    std::fs::write(bench_output_path(), &json)?;
    Ok(())
}
