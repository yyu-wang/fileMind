//! Sidecar 握手协议与请求签名：HMAC-SHA256 + nonce + 递增序号防重放。
//!
//! 安全映射：
//! - S-01（Sidecar 端口冒充）：启动握手验证 Sidecar 身份
//! - T-01（Sidecar 通信篡改）：每个请求附带 HMAC 签名 + 递增序号
//!
//! 协议流程：
//! 1. Rust 生成 PSK（32 字节随机）→ spawn Sidecar 时通过 stdin 注入
//! 2. Sidecar 从 stdin 读取 PSK 存入全局
//! 3. Rust 生成 nonce → POST /handshake（带 HMAC 签名头）
//! 4. Sidecar 验签 → 缓存 nonce → 返回 proof（用 PSK 对 "handshake-ok|{nonce}" 签名）
//! 5. Rust 验证 proof → 握手成功
//! 6. 后续每个请求附带 X-Signature + X-Request-Seq，Sidecar 中间件验签 + 检查 seq 递增

use hmac::{Hmac, KeyInit, Mac};
use rand::rngs::SysRng;
use rand::TryRng;
use sha2::Sha256;

use crate::error::{AppError, AppResult};

/// HMAC-SHA256 类型别名。
type HmacSha256 = Hmac<Sha256>;

/// 签名头名称（HTTP header），承载 HMAC-SHA256 hex 签名。
pub const SIGNATURE_HEADER: &str = "X-Signature";
/// 请求序号头名称（HTTP header），用于防重放。
pub const REQUEST_SEQ_HEADER: &str = "X-Request-Seq";
/// PSK 长度（字节）。
pub const PSK_LEN: usize = 32;
/// Nonce 长度（字节）。
pub const NONCE_LEN: usize = 32;

/// 生成指定长度的随机字节。
///
/// 使用 OS 随机源（`SysRng`）避免 thread-local RNG 状态共享，
/// 并通过 fallible `try_fill_bytes` 防止熵源失败时 panic。
fn random_bytes<const N: usize>() -> AppResult<[u8; N]> {
    let mut bytes = [0u8; N];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| AppError::HandshakeFailed(format!("随机数生成失败: {e}")))?;
    Ok(bytes)
}

/// 生成预共享密钥（PSK）：每次启动新生成，通过 stdin 注入 Sidecar。
///
/// # Errors
///
/// 仅在系统随机数源失败时返回 `HandshakeFailed`。
pub fn generate_psk() -> AppResult<Vec<u8>> {
    Ok(random_bytes::<PSK_LEN>()?.to_vec())
}

/// 生成握手 nonce（hex 编码，64 字符）。
///
/// # Errors
///
/// 仅在系统随机数源失败时返回 `HandshakeFailed`。
pub fn generate_nonce() -> AppResult<String> {
    Ok(hex::encode(random_bytes::<NONCE_LEN>()?))
}

/// 计算 HMAC-SHA256 签名（hex 编码）。
///
/// # Errors
///
/// PSK 长度无效（HMAC 实现要求 key 满足块大小约束）时返回 `HandshakeFailed`。
pub fn sign(psk: &[u8], message: &str) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(psk)
        .map_err(|e| AppError::HandshakeFailed(format!("PSK 长度无效: {e}")))?;
    mac.update(message.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// 验证签名（常量时间比较，防时序攻击）。
///
/// # Errors
///
/// 签名 hex 解码失败返回 `RequestSignatureInvalid`；签名验证失败（不匹配）返回 `RequestSignatureInvalid`。
pub fn verify(psk: &[u8], message: &str, signature_hex: &str) -> AppResult<()> {
    let provided = hex::decode(signature_hex)
        .map_err(|_| AppError::RequestSignatureInvalid("签名 hex 解码失败".into()))?;

    let mut mac = HmacSha256::new_from_slice(psk)
        .map_err(|e| AppError::HandshakeFailed(format!("PSK 长度无效: {e}")))?;
    mac.update(message.as_bytes());
    mac.verify_slice(&provided)
        .map_err(|_| AppError::RequestSignatureInvalid("签名验证失败".into()))
}

/// 构造握手请求签名 canonical string。
///
/// 格式：`handshake|{nonce_hex}`
#[must_use]
pub fn build_handshake_canonical(nonce_hex: &str) -> String {
    format!("handshake|{nonce_hex}")
}

/// 构造握手响应 proof canonical string。
///
/// 格式：`handshake-ok|{nonce_hex}`
#[must_use]
pub fn build_handshake_proof_canonical(nonce_hex: &str) -> String {
    format!("handshake-ok|{nonce_hex}")
}

/// 构造请求签名 canonical string。
///
/// 格式：`{method}|{path}|{body}|{seq}`
#[must_use]
pub fn build_request_canonical(method: &str, path: &str, body: &str, seq: u64) -> String {
    format!("{method}|{path}|{body}|{seq}")
}

/// 执行握手协议：POST /handshake 验证 Sidecar 身份。
///
/// 流程：发送 nonce + 签名头 → 接收 proof → 用 PSK 验证 proof。
///
/// # Errors
///
/// 网络请求失败返回 `SidecarUnavailable`；HTTP 非 2xx、proof 缺失或验签失败返回 `HandshakeFailed`。
pub async fn perform_handshake(base_url: &str, psk: &[u8], nonce_hex: &str) -> AppResult<()> {
    let canonical = build_handshake_canonical(nonce_hex);
    let signature = sign(psk, &canonical)?;
    let body = serde_json::json!({ "nonce": nonce_hex }).to_string();

    let url = format!("{base_url}/handshake");
    let resp = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .header(SIGNATURE_HEADER, &signature)
        .body(body)
        .send()
        .await
        .map_err(|e| AppError::SidecarUnavailable(format!("握手请求失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::HandshakeFailed(format!(
            "握手失败，HTTP 状态: {}",
            resp.status()
        )));
    }

    let proof_response: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::HandshakeFailed(format!("握手响应解析失败: {e}")))?;

    let proof = proof_response
        .get("proof")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::HandshakeFailed("握手响应缺少 proof 字段".into()))?;

    let proof_canonical = build_handshake_proof_canonical(nonce_hex);
    verify(psk, &proof_canonical, proof)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_psk_length() -> AppResult<()> {
        let psk = generate_psk()?;
        assert_eq!(psk.len(), PSK_LEN);
        Ok(())
    }

    #[test]
    fn test_generate_psk_uniqueness() -> AppResult<()> {
        let a = generate_psk()?;
        let b = generate_psk()?;
        assert_ne!(a, b, "两次生成的 PSK 不应相同");
        Ok(())
    }

    #[test]
    fn test_generate_nonce_hex_length() -> AppResult<()> {
        let nonce = generate_nonce()?;
        assert_eq!(nonce.len(), NONCE_LEN * 2, "hex 编码后长度应为字节长度 ×2");
        assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
        Ok(())
    }

    #[test]
    fn test_sign_and_verify_roundtrip() -> AppResult<()> {
        let psk = generate_psk()?;
        let message = "handshake|abc123";
        let signature = sign(&psk, message)?;
        verify(&psk, message, &signature)?;
        Ok(())
    }

    #[test]
    fn test_verify_rejects_wrong_signature() -> AppResult<()> {
        let psk = generate_psk()?;
        let message = "handshake|abc123";
        // 64 字符全 0 签名（长度合法但内容不匹配）
        let wrong_sig = "0".repeat(64);
        let result = verify(&psk, message, &wrong_sig);
        assert!(result.is_err(), "错误签名应被拒绝");
        Ok(())
    }

    #[test]
    fn test_verify_rejects_different_psk() -> AppResult<()> {
        let psk_a = generate_psk()?;
        let psk_b = generate_psk()?;
        let message = "handshake|abc123";
        // 用 PSK A 签名，用 PSK B 验证 → 必须失败
        let signature = sign(&psk_a, message)?;
        let result = verify(&psk_b, message, &signature);
        assert!(result.is_err(), "不同 PSK 签名应被拒绝");
        Ok(())
    }

    #[test]
    fn test_verify_rejects_invalid_hex() -> AppResult<()> {
        let psk = generate_psk()?;
        let message = "handshake|abc123";
        let result = verify(&psk, message, "not-a-hex-string");
        assert!(result.is_err(), "非法 hex 签名应被拒绝");
        Ok(())
    }

    #[test]
    fn test_verify_rejects_empty_signature() -> AppResult<()> {
        let psk = generate_psk()?;
        let message = "handshake|abc123";
        let result = verify(&psk, message, "");
        assert!(result.is_err(), "空签名应被拒绝");
        Ok(())
    }

    #[test]
    fn test_build_handshake_canonical_format() {
        let canonical = build_handshake_canonical("deadbeef");
        assert_eq!(canonical, "handshake|deadbeef");
    }

    #[test]
    fn test_build_handshake_proof_canonical_format() {
        let canonical = build_handshake_proof_canonical("deadbeef");
        assert_eq!(canonical, "handshake-ok|deadbeef");
    }

    #[test]
    fn test_build_request_canonical_format() {
        let canonical = build_request_canonical("POST", "/classify", "{\"x\":1}", 42);
        assert_eq!(canonical, "POST|/classify|{\"x\":1}|42");
    }

    #[test]
    fn test_sign_deterministic_for_same_input() -> AppResult<()> {
        let psk = generate_psk()?;
        let message = "test-message";
        let sig_a = sign(&psk, message)?;
        let sig_b = sign(&psk, message)?;
        assert_eq!(sig_a, sig_b, "相同输入应产生相同签名");
        Ok(())
    }

    #[test]
    fn test_proof_roundtrip_simulation() -> AppResult<()> {
        // 模拟完整握手协议的算法部分（不发网络请求）
        let psk = generate_psk()?;
        let nonce = generate_nonce()?;

        // Rust 端构造握手签名
        let client_canonical = build_handshake_canonical(&nonce);
        let client_sig = sign(&psk, &client_canonical)?;

        // Sidecar 端验证客户端签名
        verify(&psk, &client_canonical, &client_sig)?;

        // Sidecar 端构造 proof
        let proof_canonical = build_handshake_proof_canonical(&nonce);
        let proof = sign(&psk, &proof_canonical)?;

        // Rust 端验证 proof
        verify(&psk, &proof_canonical, &proof)?;
        Ok(())
    }
}
