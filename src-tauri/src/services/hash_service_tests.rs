//! services::hash_service 单元测试：与 Python hash_service.py 的行为逐一对齐。
//!
//! 覆盖：空文件 hash、小文件已知值、大文件流式读取、不存在文件错误回包、
//! hex 输出格式（64 字符、全小写、全 hex 字符集）。

use super::hash_service::compute_file_hash;
use std::io::Write;
use std::path::PathBuf;

/// 生成临时文件 + 返回路径（用完自动删）。
fn create_temp_file(
    name: &str,
    content: &[u8],
) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join(name);
    let mut f = std::fs::File::create(&path)?;
    f.write_all(content)?;
    Ok((tmp, path))
}

#[test]
fn test_empty_file_hash_matches_python_hashlib() -> Result<(), Box<dyn std::error::Error>> {
    // Python hashlib.sha256(b"").hexdigest() 的标准值
    let (_tmp, path) = create_temp_file("empty.bin", b"")?;
    let h = compute_file_hash(&path)?;
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    Ok(())
}

#[test]
fn test_small_file_known_value() -> Result<(), Box<dyn std::error::Error>> {
    // Python: hashlib.sha256(b"hello").hexdigest()
    let (_tmp, path) = create_temp_file("hello.txt", b"hello")?;
    let h = compute_file_hash(&path)?;
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    Ok(())
}

#[test]
fn test_multichunk_streaming_no_oom() -> Result<(), Box<dyn std::error::Error>> {
    // 4MB 的重复内容（512,000 块 8 字节，但 BufReader 合并到 8KB 块）
    let size = 4 * 1024 * 1024;
    let chunk: Vec<u8> = b"abcdefgh".repeat(1024); // 8 KB

    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("large.bin");
    let mut f = std::fs::File::create(&path)?;
    for _ in 0..(size / 8192) {
        f.write_all(&chunk)?;
    }
    drop(f);

    let h = compute_file_hash(&path)?;
    assert_eq!(h.len(), 64);
    // 用 Python 做一次对等计算：repeat 8KB abcdefgh * 512 次 → 4MB
    // Python: hashlib.sha256(b"abcdefgh" * (8192/8) * (4MB/8192)).hexdigest()
    // 简化：不硬编码 hash 具体值，只验证格式正确 + 两次结果一致（稳定）
    let h2 = compute_file_hash(&path)?;
    assert_eq!(h, h2, "相同文件两次 hash 不一致");
    Ok(())
}

#[test]
fn test_nonexistent_file_returns_err() {
    let bogus = PathBuf::from("/definitely/does/not/exist_xyz_123");
    let result = compute_file_hash(&bogus);
    assert!(result.is_err(), "不存在的文件应该返回 Err");
}

#[test]
fn test_hash_is_64_hex_lowercase() -> Result<(), Box<dyn std::error::Error>> {
    // 各种典型字节内容，验证 hex 输出字符集严格在 [0-9a-f]
    for content in [
        b"abc" as &[u8],
        b"\x00\x01\x02\xff",
        // "Hello 中文 世界" 的 UTF-8 字节序列（Rust byte string literal 不允许非 ASCII）
        &b"Hello \xe4\xb8\xad\xe6\x96\x87 \xe4\xb8\x96\xe7\x95\x8c \n\t"[..],
        &[0u8; 1024],
    ] {
        let (_tmp, path) = create_temp_file("t.bin", content)?;
        let h = compute_file_hash(&path)?;
        assert_eq!(h.len(), 64, "content={content:?}, hash={h}");
        assert!(
            h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "hash 字符非十六进制小写: {h}"
        );
    }
    Ok(())
}
