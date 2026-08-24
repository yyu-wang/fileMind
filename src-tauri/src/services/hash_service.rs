//! Rust 端文件 SHA-256 哈希计算（与 Python sidecar 的 `hash_service.py` 对齐）。
//!
//! 设计要点：
//! - 8 KB 分块读取，避免大文件 OOM（与 Python `hash_service.py` `CHUNK_SIZE` 完全对齐）
//! - 失败时返回 `AppError::Io`，不 panic（调用方可决定降级为 None）
//! - 空文件返回 SHA-256 空输入常量（`e3b0c442...`），与 Python hashlib 行为一致

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use hex::ToHex;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::FileInfo;

/// 分块大小：8 KB（与 Python sidecar `hash_service.py` 对齐，保持跨端 hash 一致）
const CHUNK_SIZE: usize = 8 * 1024;

/// 并行 hash 线程数上限（`available_parallelism` 超出时截断，防止小型扫描过度分片）。
const MAX_PARALLEL_HASHERS: usize = 16;

/// 计算文件 SHA-256，返回小写 hex 字符串（64 字符）。
///
/// # Errors
///
/// 文件打开/读取失败时返回 `AppError::Io`（包装底层 `std::io::Error`）。
pub fn compute_file_hash(path: &Path) -> AppResult<String> {
    // 权限敏感：这里只读取内容做 hash，不修改任何文件；BufReader 默认 8KB 缓冲
    let file = File::open(path).map_err(AppError::from)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; CHUNK_SIZE];

    loop {
        // 单次最多读 CHUNK_SIZE，避免大文件把全部内容塞进内存
        let n = reader.read(&mut buf).map_err(AppError::from)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    // sha2 0.11.x finalize 返回 GenericArray，通过 hex::ToHex trait 转小写 hex
    Ok(hasher.finalize().encode_hex::<String>())
}

/// 并行计算一批文件的 SHA-256，仅对 `needs_hash` 标记为 `true` 的文件重算
/// （未标记文件保持其既有 `content_hash`，通常已由快照复用）。
///
/// 实现：`std::thread::scope` 手写并行，不引入 rayon（规则 42，约 40 行可自写）。
/// 线程数按 `available_parallelism` 取 `1..=16`；各线程只读自己分块的路径列表，
/// 无共享可变状态。单个文件 hash 失败退化为 `None`，不阻塞整批。
///
/// 若某线程 panic（仅理论情况），该线程结果缺失时整批回填被跳过，受影响文件
/// 保持 `content_hash = None`，与「不可读文件退化为 None」语义一致。
pub fn compute_hashes_parallel(files: &mut [FileInfo], needs_hash: &[bool]) {
    debug_assert_eq!(files.len(), needs_hash.len());

    // 收集需重算的路径（保持原顺序）；空则直接返回
    let pending: Vec<PathBuf> = files
        .iter()
        .zip(needs_hash)
        .filter(|(_, &need)| need)
        .map(|(f, _)| PathBuf::from(&f.path))
        .collect();
    if pending.is_empty() {
        return;
    }

    let worker_count = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, MAX_PARALLEL_HASHERS);
    let chunk_size = pending.len().div_ceil(worker_count);

    let results: Vec<Option<String>> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for chunk in pending.chunks(chunk_size) {
            let chunk = chunk.to_vec();
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|p| compute_file_hash(p).ok())
                    .collect::<Vec<_>>()
            }));
        }
        // join 顺序与 spawn 顺序一致 → 结果顺序与 pending 顺序一致
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });

    // 数量对不上（某线程 panic）→ 跳过回填，受影响文件保持 None，避免错位
    if results.len() != pending.len() {
        return;
    }
    let mut it = results.into_iter();
    for (f, &need) in files.iter_mut().zip(needs_hash) {
        if need {
            f.content_hash = it.next().flatten();
        }
    }
}
