//! 文件预览命令：按扩展名读取文件内容供前端预览（文本 / 图片 / PDF）。
//!
//! 安全模型：所有路径先经 `security::validate`（规范化 + 系统目录黑名单拦截），
//! 前端无法绕过校验直接读取任意文件。读取内容带字节上限，防止超大文件撑爆内存
//! 与 IPC 传输。

use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::security;
use crate::sidecar::proxy;
use crate::AppState;

/// 文本预览上限（字节）。超出时截断并标记 `truncated=true`。
const MAX_TEXT_BYTES: u64 = 512 * 1024;
/// 图片预览上限（字节）。超出返回 `FILE-E-004`，不降级截断（图片截断无法显示）。
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// PDF 预览上限（字节）。超出返回 `FILE-E-004`。
const MAX_PDF_BYTES: u64 = 30 * 1024 * 1024;

/// 预览内容类型，前端按此渲染。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum PreviewKind {
    /// 纯文本（含 markdown / 代码等），`text` 字段有值。
    Text,
    /// 图片，`data_url` 字段有值。
    Image,
    /// PDF，`data_url` 字段有值。
    Pdf,
    /// 不支持的扩展名，仅返回元信息。
    Unsupported,
}

/// 文件预览响应（IPC 传输视图）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FilePreview {
    /// 内容类型。
    pub kind: PreviewKind,
    /// 文件名。
    pub file_name: String,
    /// 文件大小（字节）。
    #[specta(type = specta_typescript::Number)]
    pub file_size: u64,
    /// 文本内容（仅 `kind=Text`）。
    pub text: Option<String>,
    /// data URL（仅 `kind=Image/Pdf`，形如 `data:image/png;base64,...`）。
    pub data_url: Option<String>,
    /// 文本超限截断标记（仅 `kind=Text`）。
    pub truncated: bool,
}

/// 读取文件内容供前端预览（API 规格书 §2-2e 前置能力）。
///
/// 路径经 `security::validate` 校验；按扩展名分派到文本 / 图片 / PDF / 不支持。
///
/// # Errors
///
/// 路径不安全返回 `UnsafePath`；文件不存在或不是普通文件返回 `FILE-E-002`；
/// 读取失败返回 `FILE-E-003`；超过类型对应字节上限返回 `FILE-E-004`。
#[tauri::command(async)]
#[specta::specta]
pub fn read_file_preview(path: String) -> Result<FilePreview, String> {
    read_file_preview_inner(&path).map_err(|e| e.to_string())
}

/// 预览逻辑纯函数入口（便于单元测试，不依赖 `tauri::State`）。
fn read_file_preview_inner(path: &str) -> AppResult<FilePreview> {
    // 路径安全校验（规范化 + 黑名单拦截），拒绝不可达/系统目录路径
    let safe_path = security::validate(path)?;

    if !safe_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "FILE-E-002:预览对象不存在或不是文件: {}",
            safe_path.display()
        )));
    }

    let meta = std::fs::metadata(&safe_path).map_err(read_error)?;
    let file_name = safe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let (kind, mime) = classify_extension(&file_name);

    let mut preview = FilePreview {
        kind,
        file_name,
        file_size: meta.len(),
        text: None,
        data_url: None,
        truncated: false,
    };

    match preview.kind {
        PreviewKind::Text => {
            let (text, truncated) = read_text_preview(&safe_path)?;
            preview.text = Some(text);
            preview.truncated = truncated;
        }
        PreviewKind::Image | PreviewKind::Pdf => {
            let max_bytes = match preview.kind {
                PreviewKind::Image => MAX_IMAGE_BYTES,
                PreviewKind::Pdf => MAX_PDF_BYTES,
                _ => unreachable!( /* 上面已 match 到 Image|Pdf 之一 */ ),
            };
            let bytes = read_bounded_bytes(&safe_path, max_bytes)?;
            let mime = mime.unwrap_or("application/octet-stream");
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            preview.data_url = Some(format!("data:{mime};base64,{encoded}"));
        }
        PreviewKind::Unsupported => {}
    }

    Ok(preview)
}

/// 文件名扩展名是否为 Office 文档（docx / xlsx / pptx）。
///
/// 这些格式预览需经 Sidecar `doc_extract` 抽取为纯文本（PDF 走原生 data URL）。
fn is_office_document(file_name: &str) -> bool {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("docx" | "xlsx" | "pptx"))
}

/// 读取 Office 文档预览：经 Sidecar `/extract/document` 抽取为纯文本后返回。
///
/// 与文本预览同构（`kind=Text`），前端复用 `<pre>` 渲染；原始排版（表格/分页）
/// 会失真，属预期降级。路径先经 `security::validate` 校验，再转发给本机
/// Sidecar（HMAC 验签），文件内容不直接暴露给任意调用方。
///
/// # Errors
///
/// 路径不安全返回 `UnsafePath`；文件不存在返回 `FILE-E-002`；扩展名不受支持
/// 返回 `FILE-E-005`；Sidecar 未就绪或抽取失败返回对应错误。
#[tauri::command]
#[specta::specta]
pub async fn read_document_preview(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<FilePreview, String> {
    read_document_preview_async(&path, &state)
        .await
        .map_err(|e| e.to_string())
}

/// Office 文档预览逻辑入口（async：需向 Sidecar 转发抽取请求）。
async fn read_document_preview_async(path: &str, state: &AppState) -> AppResult<FilePreview> {
    let safe_path = security::validate(path)?;
    if !safe_path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "FILE-E-002:预览对象不存在或不是文件: {}",
            safe_path.display()
        )));
    }
    let meta = std::fs::metadata(&safe_path).map_err(read_error)?;
    let file_name = safe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    if !is_office_document(&file_name) {
        return Err(AppError::InvalidInput(
            "FILE-E-005:暂不支持该文档类型".into(),
        ));
    }

    // Sidecar 握手成功后 PSK 必然存在；缺失视为未就绪
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::Internal(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("Sidecar 未就绪，无法抽取文档文本".into()))?;
    let seq = state.request_seq.fetch_add(1, Ordering::SeqCst);
    let payload = serde_json::json!({ "path": safe_path.to_string_lossy() }).to_string();
    let resp = proxy::forward_post("/extract/document", &payload, &psk, seq).await?;
    let parsed: serde_json::Value = serde_json::from_str(&resp).map_err(AppError::Serialize)?;
    let text = parsed
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    Ok(FilePreview {
        kind: PreviewKind::Text,
        file_name,
        file_size: meta.len(),
        text: Some(text),
        data_url: None,
        truncated: false,
    })
}

/// 文本上限的 `usize` 视图。
///
/// 常量 `MAX_TEXT_BYTES` 声明为 `u64` 以对齐 `metadata.len()`；512KB 必然在 `usize`
/// 范围内，`unwrap_or(usize::MAX)` 仅满足类型约束、实际永不触发。
fn text_cap_usize() -> usize {
    usize::try_from(MAX_TEXT_BYTES).unwrap_or(usize::MAX)
}

/// 读取文本内容，超限截断到 [`MAX_TEXT_BYTES`] 并返回截断标记。
///
/// UTF-8 非法字节用 `from_utf8_lossy` 降级（二进制误判为文本时仍可预览部分内容）。
fn read_text_preview(path: &Path) -> AppResult<(String, bool)> {
    let file = std::fs::File::open(path).map_err(read_error)?;
    let cap = text_cap_usize();
    let mut buf = Vec::with_capacity(cap);
    file.take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(read_error)?;

    let truncated = buf.len() > cap;
    if truncated {
        buf.truncate(cap);
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

/// 读取二进制内容（图片 / PDF），文件超过上限时返回 `FILE-E-004`。
fn read_bounded_bytes(path: &Path, max_bytes: u64) -> AppResult<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(read_error)?;
    if meta.len() > max_bytes {
        return Err(AppError::InvalidInput(format!(
            "FILE-E-004:文件过大（{} 字节），超出预览上限 {} 字节",
            meta.len(),
            max_bytes
        )));
    }
    std::fs::read(path).map_err(read_error)
}

/// 按扩展名分派预览类型，返回 `(类型, 对应 MIME)`。
///
/// MIME 仅对图片 / PDF 有意义（拼 data URL）；文本与不支持类型为 `None`。
fn classify_extension(file_name: &str) -> (PreviewKind, Option<&'static str>) {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);

    match ext.as_deref() {
        Some(
            "txt" | "md" | "log" | "json" | "yaml" | "yml" | "csv" | "xml" | "toml" | "ini"
            | "conf" | "ts" | "tsx" | "js" | "jsx" | "py" | "rs" | "go" | "java" | "c" | "h"
            | "cpp" | "css" | "html" | "sh" | "sql",
        ) => (PreviewKind::Text, None),
        Some("png") => (PreviewKind::Image, Some("image/png")),
        Some("jpg" | "jpeg") => (PreviewKind::Image, Some("image/jpeg")),
        Some("gif") => (PreviewKind::Image, Some("image/gif")),
        Some("webp") => (PreviewKind::Image, Some("image/webp")),
        Some("svg") => (PreviewKind::Image, Some("image/svg+xml")),
        Some("bmp") => (PreviewKind::Image, Some("image/bmp")),
        Some("ico") => (PreviewKind::Image, Some("image/x-icon")),
        Some("pdf") => (PreviewKind::Pdf, Some("application/pdf")),
        _ => (PreviewKind::Unsupported, None),
    }
}

/// 统一 IO 错误 → 用户可读错误码（`FILE-E-003`）。
fn read_error(e: std::io::Error) -> AppError {
    AppError::InvalidInput(format!("FILE-E-003:文件读取失败 ({e})"))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::redundant_clone,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening
)]
mod tests {
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
}
