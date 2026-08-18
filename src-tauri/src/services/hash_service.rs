//! Rust 端文件 SHA-256 哈希计算（与 Python sidecar 的 `hash_service.py` 对齐）。
//!
//! 设计要点：
//! - 8 KB 分块读取，避免大文件 OOM（与 Python `hash_service.py` `CHUNK_SIZE` 完全对齐）
//! - 失败时返回 `AppError::Io`，不 panic（调用方可决定降级为 None）
//! - 空文件返回 SHA-256 空输入常量（`e3b0c442...`），与 Python hashlib 行为一致

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use hex::ToHex;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// 分块大小：8 KB（与 Python sidecar `hash_service.py` 对齐，保持跨端 hash 一致）
const CHUNK_SIZE: usize = 8 * 1024;

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
