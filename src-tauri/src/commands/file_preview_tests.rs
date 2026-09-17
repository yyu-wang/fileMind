//! `file_preview` 命令的单元测试（`use super::*` 可访问父模块私有项）。
//!
//! 独立文件拆分原因：内嵌在 `file_preview.rs` 会超 Rust 模块行数阈值
//! （rules/complexity.md），与本仓既有约定一致（见 `db/file_repo_tests.rs`）。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]

use super::*;
use std::path::PathBuf;

/// 在临时目录写一个文件，返回 `(TempDir, 文件路径)`。
///
/// 调用方必须持有 `TempDir` 直到断言结束（否则目录被删，路径失效）。
fn write_temp_file(
    name: &str,
    bytes: &[u8],
) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join(name);
    std::fs::write(&path, bytes)?;
    Ok((tmp, path))
}

#[test]
fn test_text_preview() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, path) = write_temp_file("notes.md", "hello 世界".as_bytes())?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Text);
    assert_eq!(preview.text.as_deref(), Some("hello 世界"));
    assert!(!preview.truncated);
    assert!(preview.data_url.is_none());
    Ok(())
}

#[test]
fn test_text_preview_truncates_oversize() -> Result<(), Box<dyn std::error::Error>> {
    let content = vec![b'a'; text_cap_usize() + 1000];
    let (_tmp, path) = write_temp_file("big.log", &content)?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Text);
    assert!(preview.truncated);
    let text = preview.text.ok_or("text 应非空")?;
    assert_eq!(text.len(), text_cap_usize());
    Ok(())
}

#[test]
fn test_image_preview_data_url() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, path) = write_temp_file("photo.png", &[0x89, 0x50, 0x4e, 0x47])?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Image);
    let data_url = preview.data_url.ok_or("data_url 应非空")?;
    assert!(
        data_url.starts_with("data:image/png;base64,"),
        "PNG 应生成 image/png data URL: {data_url}"
    );
    assert!(preview.text.is_none());
    Ok(())
}

#[test]
fn test_pdf_preview_data_url() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, path) = write_temp_file("doc.pdf", b"%PDF-1.4 fake")?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Pdf);
    let data_url = preview.data_url.ok_or("data_url 应非空")?;
    assert!(
        data_url.starts_with("data:application/pdf;base64,"),
        "PDF 应生成 application/pdf data URL"
    );
    Ok(())
}

#[test]
fn test_unsupported_extension() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, path) = write_temp_file("archive.zip", b"PK\x03\x04")?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Unsupported);
    assert!(preview.text.is_none());
    assert!(preview.data_url.is_none());
    Ok(())
}

#[test]
fn test_extension_case_insensitive() -> Result<(), Box<dyn std::error::Error>> {
    let (_tmp, path) = write_temp_file("README.MD", b"# title")?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Text);
    Ok(())
}

#[test]
fn test_oversize_image_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let size = usize::try_from(MAX_IMAGE_BYTES).unwrap_or(usize::MAX) + 1;
    let (_tmp, path) = write_temp_file("huge.png", &vec![0u8; size])?;
    let result = read_file_preview_inner(&path.to_string_lossy());
    let err = result.expect_err("超大图片应报错").to_string();
    assert!(err.contains("FILE-E-004"), "应返回 FILE-E-004: {err}");
    Ok(())
}

#[test]
fn test_nonexistent_path_rejected() {
    let result = read_file_preview_inner("/nonexistent/no-such-file.txt");
    assert!(result.is_err());
}

#[test]
fn test_blocked_system_path_rejected() {
    let result = read_file_preview_inner("/System/Library/CoreServices/SystemVersion.plist");
    assert!(result.is_err());
}

#[test]
fn test_file_name_empty_falls_back_to_default() -> Result<(), Box<dyn std::error::Error>> {
    // 无扩展名文件 → Unsupported，不 panic
    let (_tmp, path) = write_temp_file("README", b"plain")?;
    let preview = read_file_preview_inner(&path.to_string_lossy())?;
    assert_eq!(preview.kind, PreviewKind::Unsupported);
    assert_eq!(preview.file_name, "README");
    Ok(())
}

#[test]
fn test_is_office_document_recognizes_doc_exts() {
    assert!(is_office_document("report.docx"));
    assert!(is_office_document("sheet.XLSX"));
    assert!(is_office_document("deck.pptx"));
    assert!(!is_office_document("notes.md"));
    assert!(!is_office_document("doc.pdf"));
    assert!(!is_office_document("README"));
}
