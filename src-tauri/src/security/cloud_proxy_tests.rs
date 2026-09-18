//! `cloud_proxy` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（rules/complexity.md），
//! 与本仓既有约定一致（见 `sidecar/manager_tests/` 目录）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::cloud_proxy_server::{forward_provider_call, proxy_chat_completions};
use super::*;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// fn 指针类型要求与 security::get_key 一致（Result 包裹），此处签名被强制
#[allow(clippy::unnecessary_wraps)]
fn fake_key_reader(_provider: &str) -> AppResult<Option<String>> {
    Ok(Some("sk-test-key-abcdefghijklmnop".to_string()))
}

#[allow(clippy::unnecessary_wraps)]
fn no_key_reader(_provider: &str) -> AppResult<Option<String>> {
    Ok(None)
}

fn make_state(reader: fn(&str) -> AppResult<Option<String>>, upstream: &str) -> CloudProxyState {
    CloudProxyState::with_fakes("tok-123".to_string(), reader, upstream.to_string())
}

fn authed_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-filemind-token", "tok-123".parse().unwrap());
    headers
}

/// 简易本地上游：读一个 HTTP 请求（含 body），记录并回罐头响应。
async fn spawn_fake_upstream() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(String, String)>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (head, body) = read_full_request(&mut socket).await;
        let _ = tx.send((head, body));
        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 20\r\n\r\n{\"status\":\"ok\",\"x\":1}";
        let _ = socket.write_all(resp.as_bytes()).await;
    });
    (format!("http://{addr}"), rx)
}

/// 读到请求头 + content-length 指定 body 完整为止。
async fn read_full_request(socket: &mut tokio::net::TcpStream) -> (String, String) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let raw = String::from_utf8_lossy(&buf);
        if let Some(idx) = raw.find("\r\n\r\n") {
            // 头已完整：解析 content-length 判断 body 是否到齐（头+body 同包才 break）
            let lower_head = raw[..idx].to_ascii_lowercase();
            let content_length = lower_head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len().saturating_sub(idx + 4) >= content_length {
                break;
            }
        }
    }
    let raw = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    (head.to_string(), body.to_string())
}

#[test]
fn provider_upstream_whitelist_legacy() {
    assert_eq!(
        provider_upstream_legacy("openai"),
        Some("https://api.openai.com/v1/chat/completions")
    );
    assert_eq!(
        provider_upstream_legacy("deepseek"),
        Some("https://api.deepseek.com/chat/completions")
    );
    assert_eq!(provider_upstream_legacy("unknown"), None);
    assert_eq!(provider_upstream_legacy("http://evil.example.com"), None);
}

#[test]
fn token_matches_rejects_wrong_or_missing() {
    let state = make_state(fake_key_reader, "http://unused");
    assert!(state.token_matches(&authed_headers()));
    let mut bad = HeaderMap::new();
    bad.insert("x-filemind-token", "wrong".parse().unwrap());
    assert!(!state.token_matches(&bad));
    assert!(!state.token_matches(&HeaderMap::new()));
}

#[test]
fn constant_time_eq_matches_and_mismatches() {
    assert!(constant_time_eq("tok-123", "tok-123"));
    assert!(!constant_time_eq("tok-123", "tok-124"));
    // 长度不等必须判否（长度差折叠进累积值）
    assert!(!constant_time_eq("tok-123", "tok-1234"));
    assert!(!constant_time_eq("", "a"));
    assert!(constant_time_eq("", ""));
}

#[test]
fn generate_token_is_hex_and_unique() {
    let a = generate_token().unwrap();
    let b = generate_token().unwrap();
    assert_eq!(a.len(), 64);
    assert_ne!(a, b);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn forward_injects_auth_and_passes_body() {
    let (upstream, mut rx) = spawn_fake_upstream().await;
    let body = serde_json::json!({ "model": "deepseek-chat", "messages": [] });
    let resp = forward_provider_call(&upstream, "sk-abc", body.clone(), false)
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (head, req_body) = rx.recv().await.unwrap();
    assert!(
        head.to_ascii_lowercase()
            .contains("authorization: bearer sk-abc"),
        "应注入 Authorization 头: {head}"
    );
    assert!(head
        .to_ascii_lowercase()
        .contains("content-type: application/json"));
    let parsed: Value = serde_json::from_str(&req_body).unwrap();
    assert_eq!(parsed["model"], "deepseek-chat");
}

#[tokio::test]
async fn handler_forwards_and_passes_through_response() {
    let (upstream, mut rx) = spawn_fake_upstream().await;
    let state = make_state(fake_key_reader, &upstream);
    let body =
        Bytes::from_static(br#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#);
    let resp = proxy_chat_completions(
        State(state),
        Path("openai".to_string()),
        authed_headers(),
        body,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (head, req_body) = rx.recv().await.unwrap();
    assert!(
        head.to_ascii_lowercase()
            .contains("authorization: bearer sk-test-key-abcdefghijklmnop"),
        "应注入 Keychain 读取的 Key: {head}"
    );
    assert!(req_body.contains("gpt-4o"));
}

#[tokio::test]
async fn handler_rejects_bad_token() {
    let (upstream, _rx) = spawn_fake_upstream().await;
    let state = make_state(fake_key_reader, &upstream);
    let mut bad = HeaderMap::new();
    bad.insert("x-filemind-token", "wrong".parse().unwrap());
    let resp = proxy_chat_completions(
        State(state),
        Path("openai".to_string()),
        bad,
        Bytes::from_static(b"{}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn handler_rejects_unknown_provider() {
    // 无 upstream_override：白名单在取 Key 之前生效（`foo` 非法 → 400）
    let state = CloudProxyState::new("tok-123".to_string());
    let resp = proxy_chat_completions(
        State(state),
        Path("foo".to_string()),
        authed_headers(),
        Bytes::from_static(b"{}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn handler_rejects_missing_key() {
    let (upstream, _rx) = spawn_fake_upstream().await;
    let state = make_state(no_key_reader, &upstream);
    let resp = proxy_chat_completions(
        State(state),
        Path("openai".to_string()),
        authed_headers(),
        Bytes::from_static(b"{}"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
