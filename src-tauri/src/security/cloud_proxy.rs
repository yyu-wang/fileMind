//! 云端 LLM 请求代理（安全 07-§4 / T7.4）。
//!
//! Sidecar 组装好（已脱敏的）OpenAI Chat Completions 请求体 POST 到本代理 →
//! Rust 从 Keychain 读取 Key 并注入 `Authorization: Bearer` → 转发到云端 API →
//! 原样回传响应（含 SSE 流式，JSON/流共用一条转发路径）。API Key 全程不进入
//! Python 进程内存。
//!
//! 安全措施：
//! - 共享 token 鉴权（`X-FileMind-Token`）：防本机其他进程盗用云端 API 配额
//! - 上游 URL 白名单：仅 OpenAI / DeepSeek 官方域，杜绝任意 URL 转发
//! - Key 仅作局部变量，使用后经 `zeroize` 清零再 drop（`unsafe_code=deny`）
//! - 审计日志只记 provider 与是否流式，不记 Key、不记内容（T7.1 二次兜底）

use std::sync::OnceLock;
use std::time::Duration;

use axum::body::Body;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use serde_json::Value;
use zeroize::Zeroize;

use crate::error::{AppError, AppResult};
use crate::security;

/// 代理监听地址（仅本机回环）。
pub const CLOUD_PROXY_HOST: &str = "127.0.0.1";
/// 代理监听端口（固定，与 Sidecar 8765 相邻）。
pub const CLOUD_PROXY_PORT: u16 = 8766;

/// 调用方鉴权请求头；Sidecar 经 `FILEMIND_CLOUD_PROXY_TOKEN` env 取值。
const TOKEN_HEADER: &str = "x-filemind-token";

/// 共享 HTTP Client 单例（BE-m3）：连接池/TLS 会话复用，避免每请求新建 Client。
/// 不设总超时——JSON 与 SSE 流式共用此 Client，总超时会掐断长流；
/// 非流式请求在 `RequestBuilder` 级单独设超时（见 [`forward_provider_call`]）。
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 返回共享 `reqwest::Client`（连接超时 10s，首次访问惰性初始化）。
fn client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        // build 失败仅理论上可能（TLS 后端初始化异常），退回默认 Client 保可用
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                log::warn!("云端代理 Client 构建失败，退回默认 Client: {e}");
                reqwest::Client::new()
            })
    })
}

/// 常量时间字符串比较（BE-m4）：逐字节异或累积，不因首个不匹配字节提前返回，
/// 避免逐字节计时侧信道猜测 token。长度差折叠进同一累积值，长度信息也不泄漏。
/// 回环 + 256bit 随机 token 下实际风险极低，顺手加固。
#[must_use]
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    // 先累计长度差：长度不等时仍完整跑完字节比较，总耗时与内容无关
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let byte_a = a.get(i).copied().unwrap_or(0);
        let byte_b = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(byte_a ^ byte_b);
    }
    diff == 0
}

/// 云端代理状态（启动时构造，随 axum 路由共享）。
#[derive(Clone)]
pub struct CloudProxyState {
    /// 本机调用方共享 token（仅内存 + Sidecar env 持有）。
    token: String,
    /// 测试用上游 URL 覆盖；生产为 `None`（走白名单）。
    upstream_override: Option<String>,
    /// Keychain 读取器（fn 指针，测试可注入假实现）。
    key_reader: fn(&str) -> AppResult<Option<String>>,
}

impl CloudProxyState {
    /// 生产构造：Keychain 读取器固定为 `security::get_key`。
    #[must_use]
    pub fn new(token: String) -> Self {
        Self {
            token,
            upstream_override: None,
            key_reader: security::get_key,
        }
    }

    /// 测试构造：注入假 Key 读取器与假上游 URL。
    #[cfg(test)]
    fn with_fakes(
        token: String,
        key_reader: fn(&str) -> AppResult<Option<String>>,
        upstream_override: String,
    ) -> Self {
        Self {
            token,
            upstream_override: Some(upstream_override),
            key_reader,
        }
    }

    /// 校验请求头中的共享 token（常量时间比较，BE-m4）。
    #[must_use]
    fn token_matches(&self, headers: &HeaderMap) -> bool {
        headers
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| constant_time_eq(v, &self.token))
    }

    /// 目标上游 URL：测试覆盖优先，否则走白名单。
    #[must_use]
    fn upstream_url(&self, provider: &str) -> Option<String> {
        if let Some(override_url) = &self.upstream_override {
            return Some(override_url.clone());
        }
        provider_upstream(provider).map(str::to_owned)
    }
}

/// 上游 URL 白名单（07-§4：仅官方 API，杜绝任意 URL 转发）。
#[must_use]
fn provider_upstream(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1/chat/completions"),
        "deepseek" => Some("https://api.deepseek.com/chat/completions"),
        _ => None,
    }
}

/// 生成调用方共享 token（32 字节系统熵随机 hex）。
///
/// # Errors
///
/// 系统熵源不可用时返回 `SidecarUnavailable`。
pub fn generate_token() -> AppResult<String> {
    use rand::TryRng;
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| AppError::SidecarUnavailable(format!("云端代理 token 生成失败: {e}")))?;
    Ok(hex::encode(bytes))
}

/// 转发云端请求：注入 `Authorization: Bearer` 后透传 body。
///
/// 非流式请求（`streaming=false`）设 120s 整体超时；流式不设——
/// 共享 Client 的总超时会掐断 SSE 长流（BE-m3）。
///
/// # Errors
///
/// 上游网络错误时返回 `Network`。
async fn forward_provider_call(
    upstream: &str,
    key: &str,
    body: Value,
    streaming: bool,
) -> AppResult<reqwest::Response> {
    // Bearer 串含 Key：reqwest 内部会拷贝走自己那份，这里持有的明文副本
    // 用后清零（BE-m3，与 handler 中 key.zeroize() 同层级的内存卫生）
    let mut auth_header = format!("Bearer {key}");
    let mut request = client()
        .post(upstream)
        .header(reqwest::header::AUTHORIZATION, &auth_header);
    if !streaming {
        request = request.timeout(Duration::from_mins(2));
    }
    let result = request.json(&body).send().await.map_err(AppError::Network);
    auth_header.zeroize();
    result
}

/// 云端代理端点：`POST /cloud-proxy/{provider}/chat/completions`。
///
/// 请求体为 `OpenAI` Chat Completions JSON（含 `stream: true` 时流式透传）。
async fn proxy_chat_completions(
    State(state): State<CloudProxyState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 1. 鉴权（token 不符直接拒，不进入后续任何逻辑）
    if !state.token_matches(&headers) {
        return error_response(StatusCode::UNAUTHORIZED, "代理鉴权失败");
    }
    // 2. provider 白名单（同时保证日志里只有合法 provider，防日志注入）
    let Some(upstream) = state.upstream_url(&provider) else {
        return error_response(StatusCode::BAD_REQUEST, "不支持的云服务商");
    };
    // 3. Keychain 取 Key（仅内存短驻，不传给任何子进程）
    let mut key = match (state.key_reader)(&provider) {
        Ok(Some(key)) => key,
        Ok(None) => return error_response(StatusCode::UNAUTHORIZED, "该服务商 API Key 未配置"),
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    // 4. 解析请求体 + 审计（只记 provider / 是否流式）
    let body_json: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "请求体不是合法 JSON"),
    };
    let streaming = body_json
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    log::info!("cloud.proxy: provider={provider}, stream={streaming}");

    // 5. 转发到云端（共享 Client，BE-m3）
    let upstream_resp = match forward_provider_call(&upstream, &key, body_json, streaming).await {
        Ok(resp) => resp,
        Err(e) => return error_response(StatusCode::BAD_GATEWAY, &e.to_string()),
    };
    // 6. Key 使用完毕清零（内存卫生，Key 不经过 Python）
    key.zeroize();

    // 7. 回传 status + content-type + 流式 body（JSON / SSE 共用路径）
    let mut builder = Response::builder().status(upstream_resp.status());
    if let Some(content_type) = upstream_resp.headers().get(reqwest::header::CONTENT_TYPE) {
        if let Ok(ct) = content_type.to_str() {
            builder = builder.header(reqwest::header::CONTENT_TYPE, ct);
        }
    }
    match builder.body(Body::from_stream(upstream_resp.bytes_stream())) {
        Ok(resp) => resp,
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// 错误响应：JSON 错误体 + 对应状态码。
fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": { "message": message, "type": "filemind_proxy" } });
    let body_str = body.to_string();
    Response::builder()
        .status(status)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body_str))
        // 状态码/头均为合法常量，构造失败仅理论可能，兜底空响应
        .unwrap_or_else(|_| Response::new(Body::from("")))
}

/// 启动代理服务：axum 路由挂到 tokio 后台任务，返回前完成端口绑定。
///
/// # Errors
///
/// 端口被占用等绑定失败时返回 `SidecarUnavailable`。
pub fn spawn_proxy_server(state: CloudProxyState) -> AppResult<()> {
    let listener =
        std::net::TcpListener::bind((CLOUD_PROXY_HOST, CLOUD_PROXY_PORT)).map_err(|e| {
            AppError::SidecarUnavailable(format!("云端代理端口 {CLOUD_PROXY_PORT} 绑定失败: {e}"))
        })?;
    // tokio `TcpListener::from_std` 要求 socket 已处于非阻塞模式（debug_assert 强制）。
    // 若沿用 std 默认的阻塞模式，debug 构建会在运行时内注册阻塞 fd 时 panic（tokio#7172）。
    // 保持同步绑定契约不变：此处 set_nonblocking 失败同样按 SidecarUnavailable 退出。
    listener.set_nonblocking(true).map_err(|e| {
        AppError::SidecarUnavailable(format!(
            "云端代理端口 {CLOUD_PROXY_PORT} 设为非阻塞失败: {e}"
        ))
    })?;
    let router = Router::new()
        .route(
            "/cloud-proxy/{provider}/chat/completions",
            post(proxy_chat_completions),
        )
        .with_state(state);
    tauri::async_runtime::spawn(async move {
        match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, router).await {
                    log::error!("云端代理服务异常退出: {e}");
                }
            }
            Err(e) => log::error!("云端代理 TcpListener 转 tokio 失败: {e}"),
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
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

    fn make_state(
        reader: fn(&str) -> AppResult<Option<String>>,
        upstream: &str,
    ) -> CloudProxyState {
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
    fn provider_upstream_whitelist() {
        assert_eq!(
            provider_upstream("openai"),
            Some("https://api.openai.com/v1/chat/completions")
        );
        assert_eq!(
            provider_upstream("deepseek"),
            Some("https://api.deepseek.com/chat/completions")
        );
        assert_eq!(provider_upstream("unknown"), None);
        assert_eq!(provider_upstream("http://evil.example.com"), None);
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
            head.contains("authorization: Bearer sk-abc"),
            "应注入 Authorization 头: {head}"
        );
        assert!(head.contains("content-type: application/json"));
        let parsed: Value = serde_json::from_str(&req_body).unwrap();
        assert_eq!(parsed["model"], "deepseek-chat");
    }

    #[tokio::test]
    async fn handler_forwards_and_passes_through_response() {
        let (upstream, mut rx) = spawn_fake_upstream().await;
        let state = make_state(fake_key_reader, &upstream);
        let body = Bytes::from_static(
            br#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
        );
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
            head.contains("authorization: Bearer sk-test-key-abcdefghijklmnop"),
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
}
