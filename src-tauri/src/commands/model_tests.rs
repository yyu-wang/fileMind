//! `commands::model` 单元测试：状态/导入响应解析、请求体字段名与离线包路径安全校验。
//!
//! 独立文件拆分原因：内嵌与实现合计超 Rust 模块 300 行警告阈值；与
//! `commands/classify_tests.rs`、`commands/file_preview_tests.rs` 同款。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

#[test]
fn status_path_carries_model_name_as_query() {
    assert_eq!(
        status_path("bge-large-zh-v1.5"),
        "/models/download/status?model_name=bge-large-zh-v1.5"
    );
}

#[test]
fn parse_status_downloading_with_progress() {
    let body = r#"{
            "model_name": "bge-large-zh-v1.5",
            "status": "downloading",
            "mirror": "https://hf-mirror.com",
            "attempt": 1,
            "downloaded_bytes": 1048576,
            "total_bytes": 327363707,
            "error": null,
            "updated_at": "2026-09-15T10:00:00"
        }"#;
    let status = parse_status(body).unwrap();
    assert_eq!(status.status, "downloading");
    assert_eq!(status.mirror.as_deref(), Some("https://hf-mirror.com"));
    assert_eq!(status.attempt, 1);
    assert_eq!(status.downloaded_bytes, 1_048_576);
    assert_eq!(status.total_bytes, Some(327_363_707));
    assert!(status.error.is_none());
}

#[test]
fn parse_status_failed_with_error_and_unknown_total() {
    let body = r#"{
            "model_name": "bge-large-zh-v1.5",
            "status": "failed",
            "mirror": "https://hf-mirror.com",
            "attempt": 3,
            "downloaded_bytes": 0,
            "total_bytes": null,
            "error": "下载失败（已自动重试 3 次）",
            "updated_at": "2026-09-15T10:05:00"
        }"#;
    let status = parse_status(body).unwrap();
    assert_eq!(status.status, "failed");
    assert_eq!(status.attempt, 3);
    assert_eq!(status.total_bytes, None);
    assert!(status.error.unwrap().contains("3 次"));
}

#[test]
fn parse_status_rejects_malformed_json() {
    assert!(parse_status(r#"{"status": 1}"#).is_err());
}

/// 请求体字段名与 Sidecar `ModelImportRequest` 一致（HTTP 层契约）。
#[test]
fn import_body_uses_sidecar_field_name() {
    let body = import_body("/tmp/offline.zip");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["path"], "/tmp/offline.zip");
}

/// 导入响应解析：导入与跳过明细均透传。
#[test]
fn parse_import_result_reads_both_lists() {
    let body = r#"{"imported": ["bge-large-zh-v1.5"], "skipped": ["bge-reranker-v2-m3"]}"#;
    let result = parse_import_result(body).unwrap();
    assert_eq!(result.imported, vec!["bge-large-zh-v1.5"]);
    assert_eq!(result.skipped, vec!["bge-reranker-v2-m3"]);
}

/// 畸形导入响应 → 序列化错误（不 panic）。
#[test]
fn parse_import_result_rejects_malformed_json() {
    assert!(matches!(
        parse_import_result("not-json"),
        Err(AppError::Serialize(_))
    ));
}

/// 安全红线：不存在的路径被 path_guard 拒绝（在触达 Sidecar 之前）。
#[test]
fn safe_package_path_rejects_missing() {
    let err = safe_package_path("/nonexistent/filemind-offline-package.zip").unwrap_err();
    assert!(matches!(err, AppError::UnsafePath(_)), "实际: {err:?}");
}

/// 黑名单路径被拒绝（即使存在）。
#[test]
fn safe_package_path_rejects_blocked() {
    assert!(safe_package_path("/System/filemind-offline-package.zip").is_err());
    assert!(safe_package_path("/etc/hosts").is_err());
}

/// 合法路径 → 返回 canonical 绝对路径（校验与使用同一个路径）。
#[test]
fn safe_package_path_returns_canonical() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::NamedTempFile::new()?;
    let raw = tmp.path().to_str().ok_or("non-UTF8 path")?;
    let canonical = safe_package_path(raw)?;
    assert_eq!(canonical, tmp.path().canonicalize()?.to_string_lossy());
    Ok(())
}
