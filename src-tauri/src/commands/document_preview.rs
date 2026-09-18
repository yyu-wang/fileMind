//! Office 文档预览：经 Sidecar `/extract/document` 抽取为纯文本（原 `file_preview.rs` 拆出）。
//!
//! 与本地直读路径（文本 / 图片 / PDF）分开成模块：docx / xlsx / pptx 的预览需经 Sidecar
//! `doc_extract` 抽取，涉及 PSK 校验与 HMAC 签名转发，失败语义（Sidecar 未就绪 / 抽取失败）
//! 与文件读 IO 完全不同。
//!
//! ⚠️ `#[tauri::command] read_document_preview` 仍留在 `file_preview.rs`：宏生成的隐藏辅助项
//! （`__cmd__*` / `__specta__fn__*`）只存在于命令定义所在模块（同 `commands/file_ops` 的说明），
//! 故 `ipc_handler.rs` / `bin/export_specta.rs` 的注册路径保持
//! `commands::file_preview::read_document_preview` 不变。

use std::sync::atomic::Ordering;

use crate::commands::file_preview::{is_office_document, read_error, FilePreview, PreviewKind};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::sidecar::proxy;
use crate::AppState;

/// Office 文档预览逻辑入口（async：需向 Sidecar 转发抽取请求）。
///
/// 路径先经 `security::validate` 校验，再转发给本机 Sidecar（HMAC 验签），文件内容不直接
/// 暴露给任意调用方。返回与文本预览同构（`kind=Text`），前端复用 `<pre>` 渲染；原始排版
/// （表格/分页）会失真，属预期降级。
///
/// # Errors
///
/// 路径不安全返回 `UnsafePath`；文件不存在返回 `FILE-E-002`；扩展名不受支持返回
/// `FILE-E-005`；Sidecar 未就绪或抽取失败返回对应错误。
pub(super) async fn extract_document_preview(
    path: &str,
    state: &AppState,
) -> AppResult<FilePreview> {
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
