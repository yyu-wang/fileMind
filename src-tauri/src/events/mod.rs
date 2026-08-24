//! 前端事件定义：扫描、分类、对话与 Sidecar 状态推送。

use tauri::Emitter;

pub mod types;

/// 推送聊天流式事件到前端（`chat://event`）。
///
/// Rust 代理 Sidecar `/chat/stream` SSE 帧时逐帧调用；前端通过
/// `@tauri-apps/api/event.listen('chat://event')` 订阅。事件负载为
/// [`types::ChatEventPayload`]，与 Sidecar SSE `data:` 行 JSON 一致。
///
/// 推送失败仅记录日志（窗口关闭等场景），不中断后台流式任务。
pub fn emit_chat_event(app: &tauri::AppHandle, event: &str, data: serde_json::Value) {
    let payload = types::ChatEventPayload {
        event: event.to_string(),
        data,
    };
    if let Err(e) = app.emit("chat://event", payload) {
        log::warn!("chat://event 推送失败: {e}");
    }
}
