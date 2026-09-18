//! `sidecar::proxy` 单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：父模块内嵌 tests 会超 Rust 模块行数阈值（`rules/complexity.md`，
//! 警告线 300 行），与本仓既有约定一致（见 `security/cloud_proxy_tests.rs`、
//! `sidecar/bootstrap_tests.rs`）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

/// `FastAPI` 标准错误体含 `detail` → 提取 `detail` 作为真实错误原因。
#[test]
fn sidecar_error_detail_extracts_detail_field() {
    let err = sidecar_error_detail(
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        r#"{"detail":"建立索引失败: Embedding 模型未拉取"}"#,
    );
    assert!(
        err.to_string()
            .contains("建立索引失败: Embedding 模型未拉取"),
        "错误信息应包含 detail 原文，实际: {err}"
    );
}

/// 响应体不是 JSON 且缺少 detail → 截取原文兜底。
#[test]
fn sidecar_error_detail_falls_back_to_raw_body() {
    let err = sidecar_error_detail(reqwest::StatusCode::BAD_GATEWAY, "Bad Gateway");
    assert!(
        err.to_string().contains("Bad Gateway"),
        "非 JSON 错误体应原样呈现，实际: {err}"
    );
}

/// 空响应体 → 占位提示，不 panic。
#[test]
fn sidecar_error_detail_handles_empty_body() {
    let err = sidecar_error_detail(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "");
    assert!(
        err.to_string().contains("空响应体"),
        "空响应体应给出占位提示，实际: {err}"
    );
}

/// `reqwest::Response` 可由 `http::Response` 直接转换，无需真实网络。
fn mock_response(status: u16, body: &str) -> reqwest::Response {
    http::Response::builder()
        .status(status)
        .body(body.to_string())
        .unwrap()
        .into()
}

/// BE-C5：非 2xx + `FastAPI` 错误体 → Err 提取 detail 与状态码。
#[tokio::test]
async fn stream_non_2xx_with_detail_becomes_error() {
    let resp = mock_response(401, r#"{"detail":"HMAC 验签失败"}"#);
    let err = ensure_stream_success(resp)
        .await
        .expect_err("401 应转为 Err");
    let msg = err.to_string();
    assert!(msg.contains("401"), "应包含状态码: {msg}");
    assert!(msg.contains("HMAC 验签失败"), "应提取 detail: {msg}");
}

/// 非 2xx + 非 JSON body → 截取原文兜底（不 panic、不空消息）。
#[tokio::test]
async fn stream_non_2xx_non_json_body_falls_back_to_text() {
    let resp = mock_response(503, "Service Unavailable");
    let err = ensure_stream_success(resp)
        .await
        .expect_err("503 应转为 Err");
    assert!(err.to_string().contains("503"));
    assert!(err.to_string().contains("Service Unavailable"));
}

/// 2xx 原样放行，body 未被消费（调用方继续 `bytes_stream`）。
#[tokio::test]
async fn stream_success_passes_through() {
    let resp = mock_response(200, "");
    let passed = ensure_stream_success(resp).await.expect("200 应放行");
    assert_eq!(passed.status(), reqwest::StatusCode::OK);
}

/// 导入超时必须显著长于默认值，且固定 1 小时。
///
/// 契约而非实现细节：数 GB 的包放网络共享盘时，10 分钟会在拷贝途中触顶——界面报失败
/// 而 Sidecar 其实还在拷（响应已被丢弃）。固定成常量是让「有人顺手改回 from_mins(10)」
/// 在测试里就暴露，而不是等部署机上的用户遇到。
#[test]
fn import_timeout_is_one_hour_and_longer_than_default() {
    assert_eq!(IMPORT_POST_TIMEOUT, Duration::from_hours(1));
    assert!(
        IMPORT_POST_TIMEOUT > DEFAULT_POST_TIMEOUT,
        "导入超时({IMPORT_POST_TIMEOUT:?}) 必须大于默认超时({DEFAULT_POST_TIMEOUT:?})"
    );
}
