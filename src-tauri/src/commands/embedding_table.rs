//! 向量表名解析：`documents_{model}_v{version}`。
//!
//! 背景：表名的两个组成部分各有唯一来源——**模型名**来自 `app_config`
//! （用户当前生效的 Embedding 模型），**版本号**来自 Sidecar 的模型注册表
//! （`app/core/embedding_models.py` 的 `default_version`）。
//!
//! 历史上 Rust 侧三处直接把版本号写成 `_v1`（索引构建、路径同步、向量清理），
//! 而前端是按探测结果动态拼表名——一旦注册表升版本，索引写 v1、问答查 v2，
//! 会出现「向量写了但检索永远查不到」的静默故障。本模块把三处统一到
//! :func:`resolve_vector_table`，版本号不再有第二份定义。
//!
//! 版本号通过 Sidecar `POST /inference/test` 获取（该接口已返回注册表全量
//! 模型及其版本），复用现有代理与探测解析，不新增 Sidecar 端点。

use std::sync::{Arc, Mutex};

use crate::commands::ollama::{probe_inference, OllamaStatus};
use crate::db::{ConfigRepo, Database};
use crate::error::{AppError, AppResult};
use crate::AppState;

/// 构造向量表名（命名规范见 `rules/sql.md`）。
#[must_use]
pub fn vector_table_name(model: &str, version: u32) -> String {
    format!("documents_{model}_v{version}")
}

/// 从探测结果中取指定模型的版本号（纯函数，便于单测）。
#[must_use]
pub fn version_for_model(status: &OllamaStatus, model: &str) -> Option<u32> {
    status
        .embedding_models
        .iter()
        .find(|item| item.name == model)
        .map(|item| item.version)
}

/// 解析当前向量表名（`&AppState` 入口，供 IPC 命令路径使用）。
///
/// # Errors
///
/// 读取配置失败、Sidecar 未就绪 / 探测失败，或注册表中没有该模型时返回错误。
pub async fn resolve_vector_table(state: &AppState) -> AppResult<String> {
    let psk = state
        .sidecar_psk
        .lock()
        .map_err(|e| AppError::InvalidInput(format!("PSK 锁中毒: {e}")))?
        .clone()
        .ok_or_else(|| AppError::SidecarUnavailable("sidecar 未就绪".to_string()))?;
    let seq = state
        .request_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    resolve_vector_table_parts(&state.db, &psk, seq).await
}

/// 解析当前向量表名的按部件入口（供 best-effort 后台任务使用）。
///
/// 后台任务无法持有 `&AppState`（其字段 `Mutex` 非 `Arc`，跨 `spawn` 生命周期
/// 不成立），故只接收必需的「数据库句柄 + PSK + 序号」三件套。
///
/// 注意：本函数会用 `seq` 发一次探测请求，**调用方后续的 Sidecar 请求必须另取
/// 序号**（中间件要求严格递增，复用同序号会被判重放）。
///
/// # Errors
///
/// 同 :func:`resolve_vector_table`。
pub async fn resolve_vector_table_parts(
    db: &Arc<Mutex<Database>>,
    psk: &[u8],
    seq: u64,
) -> AppResult<String> {
    let model = {
        let guard = db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        ConfigRepo::get(guard.conn())?.embedding_model
    };
    let status = probe_inference(psk, seq).await?;
    let version = version_for_model(&status, &model).ok_or_else(|| {
        AppError::InvalidInput(format!(
            "Sidecar 注册表中没有模型 {model}（无法确定向量表名）"
        ))
    })?;
    Ok(vector_table_name(&model, version))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::commands::ollama::EmbeddingModelAvailability;

    fn status_with(models: Vec<(&str, u32)>) -> OllamaStatus {
        OllamaStatus {
            available: true,
            status: "ok".to_string(),
            llm_models: Vec::new(),
            embedding_models: models
                .into_iter()
                .map(|(name, version)| EmbeddingModelAvailability {
                    name: name.to_string(),
                    dim: 1024,
                    version,
                    available: true,
                })
                .collect(),
            error_code: None,
            message: None,
        }
    }

    #[test]
    fn table_name_matches_convention() {
        assert_eq!(
            vector_table_name("bge-large-zh-v1.5", 1),
            "documents_bge-large-zh-v1.5_v1"
        );
    }

    #[test]
    fn version_lookup_finds_registered_model() {
        let status = status_with(vec![("bge-large-zh-v1.5", 3), ("other", 1)]);
        assert_eq!(version_for_model(&status, "bge-large-zh-v1.5"), Some(3));
    }

    #[test]
    fn version_lookup_returns_none_for_unknown_model() {
        let status = status_with(vec![("other", 1)]);
        assert_eq!(version_for_model(&status, "bge-large-zh-v1.5"), None);
    }

    #[test]
    fn version_lookup_handles_empty_registry() {
        let status = status_with(Vec::new());
        assert_eq!(version_for_model(&status, "bge-large-zh-v1.5"), None);
    }
}
