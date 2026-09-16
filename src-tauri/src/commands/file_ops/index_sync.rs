//! 向量索引 best-effort 同步：路径更新与按 `file_id` 删除（Sidecar 未就绪只记日志，绝不影响主流程）。

use crate::commands::embedding_table;
use crate::sidecar::proxy;
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;

/// `/index/update_paths` 请求体（对齐 sidecar `IndexPathUpdateRequest`）。
#[derive(Debug, Serialize)]
struct SidecarPathUpdateItem {
    file_id: String,
    path: String,
}

/// `/index/update_paths` 请求体。
#[derive(Debug, Serialize)]
struct SidecarPathUpdateRequest {
    table_name: String,
    mappings: Vec<SidecarPathUpdateItem>,
}

/// 尽力而为：把分类移动/撤销后的文件路径同步到向量索引。
///
/// `SQLite` 的 `files.path` 已是新路径，但 `LanceDB` 向量行里的 `file_path` 仍是旧路径
/// （索引增量逻辑只认 created/modified/deleted，无移动语义），RAG 问答引用会指向
/// 失效位置。这里经 `/index/update_paths` 原地更新 `file_path`（向量不变，不重新
/// embedding）。纯 best-effort：sidecar 未就绪或失败仅记录日志，绝不影响执行结果。
pub(super) fn spawn_index_path_sync(state: &AppState, mappings: Vec<(String, String)>) {
    if mappings.is_empty() {
        return;
    }

    // sidecar 未握手（PSK 为空，如测试环境）→ 静默跳过，不 spawn
    let psk = match state.sidecar_psk.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("索引路径同步：获取 PSK 锁中毒（跳过）: {poisoned}");
            return;
        }
    };
    let Some(psk) = psk else {
        return;
    };
    // 两个序号：表名解析要发一次探测请求，路径同步请求另取一个
    // （Sidecar 中间件要求序号严格递增，复用同一序号会被判重放）
    let seq_probe = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let db = Arc::clone(&state.db);

    let count = mappings.len();
    let mappings: Vec<SidecarPathUpdateItem> = mappings
        .into_iter()
        .map(|(file_id, path)| SidecarPathUpdateItem { file_id, path })
        .collect();

    tauri::async_runtime::spawn(async move {
        // 向量表名（模型来自配置、版本号来自 Sidecar 注册表）——best-effort：
        // 解析失败（如 Sidecar 未就绪）仅告警，不影响执行结果
        let table_name =
            match embedding_table::resolve_vector_table_parts(&db, &psk, seq_probe).await {
                Ok(name) => name,
                Err(e) => {
                    log::warn!("索引路径同步：解析向量表名失败（跳过）: {e}");
                    return;
                }
            };
        let request = SidecarPathUpdateRequest {
            table_name,
            mappings,
        };
        let body = match serde_json::to_string(&request) {
            Ok(body) => body,
            Err(e) => {
                log::warn!("索引路径同步：序列化请求失败（跳过）: {e}");
                return;
            }
        };
        match proxy::forward_post("/index/update_paths", &body, &psk, seq).await {
            Ok(_) => log::info!("索引路径已同步 {count} 个文件"),
            Err(e) => log::warn!("索引路径同步失败（不影响执行结果）: {e}"),
        }
    });
}

/// `/index/delete_by_file_ids` 请求体（对齐 sidecar）。
#[derive(Debug, Serialize)]
struct SidecarDeleteByFileIdsRequest {
    table_name: String,
    file_ids: Vec<String>,
}

/// 尽力而为：从向量索引中删除指定文件的全部向量行。
///
/// 与 `spawn_index_path_sync` 同模式：异步 spawn，失败仅告警，不影响主流程。
pub(super) fn spawn_index_delete_by_file_ids(state: &AppState, file_ids: Vec<String>) {
    if file_ids.is_empty() {
        return;
    }

    let psk = match state.sidecar_psk.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("索引向量清理：获取 PSK 锁中毒（跳过）: {poisoned}");
            return;
        }
    };
    let Some(psk) = psk else {
        return;
    };
    // 两个序号：表名解析要发一次探测请求，清理请求另取一个（复用会被判重放）
    let seq_probe = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let db = Arc::clone(&state.db);

    let count = file_ids.len();
    tauri::async_runtime::spawn(async move {
        // 向量表名（模型来自配置、版本号来自 Sidecar 注册表）——best-effort
        let table_name =
            match embedding_table::resolve_vector_table_parts(&db, &psk, seq_probe).await {
                Ok(name) => name,
                Err(e) => {
                    log::warn!("索引向量清理：解析向量表名失败（跳过）: {e}");
                    return;
                }
            };
        let request = SidecarDeleteByFileIdsRequest {
            table_name,
            file_ids,
        };
        let body = match serde_json::to_string(&request) {
            Ok(body) => body,
            Err(e) => {
                log::warn!("索引向量清理：序列化请求失败（跳过）: {e}");
                return;
            }
        };
        match proxy::forward_post("/index/delete_by_file_ids", &body, &psk, seq).await {
            Ok(_) => log::info!("索引向量已清理 {count} 个文件"),
            Err(e) => log::warn!("索引向量清理失败（不影响移除结果）: {e}"),
        }
    });
}

// 测试模块：1202 行内联测试按职责拆到 5 个文件（rules/complexity.md 的 600 行测试阈值）。
// 公共夹具在 file_ops_test_support；每个模块各自 #[cfg(test)]，不得只在首个上标注——
// 否则其余模块会进非测试构建，并因引用 cfg(test) 模块而编译失败。
