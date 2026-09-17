//! 文件预览命令：按扩展名读取文件内容供前端预览（文本 / 图片 / PDF）。
//!
//! 安全模型：所有路径先经 `security::validate`（规范化 + 系统目录黑名单拦截），
//! 前端无法绕过校验直接读取任意文件。读取内容带字节上限，防止超大文件撑爆内存
//! 与 IPC 传输。
//!
//! 拆分（原单文件 409 行，逼近 Rust 模块 500 行强制阈值）：
//!   - `commands/document_preview.rs`  Office 文档（Sidecar 抽取）路径
//!   - `commands/file_preview_tests.rs` 单元测试
//!
//! 两个 `#[tauri::command]` 留在本模块，注册路径不变。

use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::commands::document_preview::extract_document_preview;
use crate::error::{AppError, AppResult};
use crate::security;
use crate::AppState;

/// 文本预览上限（字节）。超出时截断并标记 `truncated=true`。
const MAX_TEXT_BYTES: u64 = 50 * 1024 * 1024;
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
/// 可见性为 `pub(super)`：`commands::document_preview` 的抽取入口先做同一判定。
pub(super) fn is_office_document(file_name: &str) -> bool {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("docx" | "xlsx" | "pptx"))
}

/// 读取 Office 文档预览：经 Sidecar `/extract/document` 抽取为纯文本后返回。
///
/// 实现见 `commands::document_preview::extract_document_preview`；路径校验、PSK 获取与
/// 转发细节都在那边，本命令只做入口与错误码转发。
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
    extract_document_preview(&path, &state)
        .await
        .map_err(|e| e.to_string())
}

/// 文本上限的 `usize` 视图。
///
/// 常量 `MAX_TEXT_BYTES` 声明为 `u64` 以对齐 `metadata.len()`；50MB 必然在 `usize`
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
///
/// 可见性为 `pub(super)`：`commands::document_preview` 的元数据读取复用同一文案。
pub(super) fn read_error(e: std::io::Error) -> AppError {
    AppError::InvalidInput(format!("FILE-E-003:文件读取失败 ({e})"))
}

// 单元测试移到兄弟文件 file_preview_tests.rs（原内嵌，与实现合计超 Rust 模块 300 行
// 警告阈值；与 db/file_repo_tests.rs、commands/classify_tests.rs 同款）。

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod tests;
