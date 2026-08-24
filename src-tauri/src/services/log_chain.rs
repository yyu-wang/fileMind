//! 操作日志链式哈希：SHA-256 防篡改链的生成与校验。
//!
//! 设计：
//! - 每条记录的 `chain_hash = SHA256(prev_chain || canonicalize(record))`
//! - 创世值 `GENESIS` 为空字符串，作为首条记录的前驱哈希
//! - 规范化输入只覆盖**不可变字段**，排除 `status`（`undo_batch` 会
//!   `done → undone`）与 `chain_hash`（自身），避免状态更新破坏链

use hex::ToHex;
use sha2::{Digest, Sha256};

use crate::db::models::OperationLog;

/// 链式哈希的创世值（首条记录的前驱哈希）。
pub const GENESIS: &str = "";

/// 把一条操作日志规范化为用于哈希的确定性字符串。
///
/// 字段顺序固定：`id|batch_id|operation_type|source_path|target_path|prev_hash|current_hash|created_at`。
/// 以 `|` 分隔（文件路径极少含 `|`，且 Windows 文件名禁止该字符）。
#[must_use]
pub fn canonicalize(log: &OperationLog) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        log.id,
        log.batch_id,
        log.operation_type,
        log.source_path,
        log.target_path,
        log.prev_hash,
        log.current_hash,
        log.created_at
    )
}

/// 计算单条记录的链式哈希：`SHA256(prev_chain || canonical)`，返回小写 hex。
#[must_use]
pub fn hash_record(prev_chain: &str, canonical: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_chain.as_bytes());
    hasher.update(canonical.as_bytes());
    hasher.finalize().encode_hex::<String>()
}

/// 校验链式哈希完整性，返回第一条断裂记录的索引（`None` 表示完整）。
///
/// 按传入顺序逐条重算，与存储的 `chain_hash` 比对；首次不匹配即返回。
/// 注意：哈希链检测「篡改现有记录 / 删除中间记录」，但不检测尾部截断
/// （删除最后一条时无后续记录引用它）。
#[must_use]
pub fn verify(logs: &[OperationLog]) -> Option<usize> {
    let mut prev_chain = GENESIS.to_string();
    for (index, log) in logs.iter().enumerate() {
        let expected = hash_record(&prev_chain, &canonicalize(log));
        if log.chain_hash != expected {
            return Some(index);
        }
        prev_chain = expected;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_log(id: &str, source: &str, status: &str, chain: &str) -> OperationLog {
        OperationLog {
            id: id.to_string(),
            batch_id: "b1".to_string(),
            operation_type: "move".to_string(),
            source_path: source.to_string(),
            target_path: "/dst/a.txt".to_string(),
            status: status.to_string(),
            prev_hash: "h0".to_string(),
            current_hash: "h1".to_string(),
            chain_hash: chain.to_string(),
            created_at: "2026-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn test_hash_record_deterministic() {
        let a = hash_record("prev", "data");
        let b = hash_record("prev", "data");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "SHA-256 应为 64 字符 hex: {a}");
        assert!(a.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }

    #[test]
    fn test_canonicalize_excludes_status() {
        let done = mk_log("l1", "/src/a.txt", "done", "");
        let undone = mk_log("l1", "/src/a.txt", "undone", "");
        // status 变化不影响规范化结果（否则 undo 更新状态会破坏链）
        assert_eq!(canonicalize(&done), canonicalize(&undone));
    }

    #[test]
    fn test_canonicalize_includes_source_path() {
        let a = mk_log("l1", "/src/a.txt", "done", "");
        let b = mk_log("l1", "/src/b.txt", "done", "");
        assert_ne!(canonicalize(&a), canonicalize(&b));
    }

    #[test]
    fn test_verify_empty_chain_ok() {
        assert!(verify(&[]).is_none());
    }

    #[test]
    fn test_verify_valid_chain_ok() {
        // 手工构造一条合法链：chain[i] = hash(prev, canonical(log[i]))
        let log0 = mk_log("l1", "/src/a.txt", "done", "");
        let c0 = hash_record(GENESIS, &canonicalize(&log0));
        let log0 = mk_log("l1", "/src/a.txt", "done", &c0);

        let log1 = mk_log("l2", "/src/b.txt", "done", "");
        let c1 = hash_record(&c0, &canonicalize(&log1));
        let log1 = mk_log("l2", "/src/b.txt", "done", &c1);

        assert!(verify(&[log0, log1]).is_none());
    }

    #[test]
    fn test_verify_detects_tamper() {
        let log0 = mk_log("l1", "/src/a.txt", "done", "");
        let c0 = hash_record(GENESIS, &canonicalize(&log0));
        let log0 = mk_log("l1", "/src/a.txt", "done", &c0);

        // 篡改第二条记录的 source_path，但保留原 chain_hash
        let log1 = mk_log("l2", "/src/TAMPERED.txt", "done", "stale-hash");

        assert_eq!(verify(&[log0, log1]), Some(1));
    }
}
