//! 云端代理传输层：共享 HTTP Client、请求转发、响应回传与服务启动。
//!
//! （2026-09-18 按职责拆自 `cloud_proxy.rs`，335 行超 Rust 模块警告阈值 300；
//! 代理状态 [`CloudProxyState`]、上游地址解析与鉴权仍在 [`super::cloud_proxy`]。）

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

use super::cloud_proxy::{CloudProxyState, CLOUD_PROXY_HOST, CLOUD_PROXY_PORT};
use crate::error::{AppError, AppResult};

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

/// 转发云端请求：注入 `Authorization: Bearer` 后透传 body。
///
/// 非流式请求（`streaming=false`）设 120s 整体超时；流式不设——
/// 共享 Client 的总超时会掐断 SSE 长流（BE-m3）。
///
/// # Errors
///
/// 上游网络错误时返回 `Network`。
pub(super) async fn forward_provider_call(
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
pub(super) async fn proxy_chat_completions(
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
    let mut key = match state.read_key(&provider) {
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
