//! 智能分类命令：生成分类预览计划（规则引擎 + 启发式），不执行文件系统变更。
//!
//! 执行阶段复用 T3.3 `execute_operations` / `undo_batch`（分类计划项转 `PlanItem`，
//! 携带各自目标路径 + `ConflictStrategy::Skip`），共享链式撤销，无需重复实现执行链路。

use crate::db::{CategoryRepo, FileRepo, RuleRepo};
use crate::error::{AppError, AppResult};
use crate::security;
use crate::services::classifier::{self, ClassifyPreview};
use crate::AppState;

/// 生成分类预览（设计稿 §5.1）。
///
/// 流程：
///   1. `security::validate` 校验扫描根（须存在且不在黑名单）
///   2. `file_ids` 非空；`FileRepo::get_by_ids` 反查文件
///   3. `RuleRepo::list_enabled`（优先级降序）+ `CategoryRepo::list_all`
///   4. `classifier::generate_plan` 生成计划 → `aggregate_stats` 聚合统计
///   5. 生成 `batch_id`（uuid4，供执行阶段接力）
///
/// # Errors
///
/// `scan_root` 非法返回 `UnsafePath`；`file_ids` 为空或全部无效返回 `InvalidInput`；
/// 数据库读取失败返回 `Database`。
#[tauri::command(async)]
#[specta::specta]
pub fn classify_preview(
    state: tauri::State<'_, AppState>,
    file_ids: Vec<String>,
    scan_root: String,
) -> Result<ClassifyPreview, String> {
    classify_preview_inner(&state, &file_ids, &scan_root).map_err(|e| e.to_string())
}

/// 分类预览纯逻辑入口（便于单元测试，不依赖 `tauri::State`）。
fn classify_preview_inner(
    state: &AppState,
    file_ids: &[String],
    scan_root: &str,
) -> AppResult<ClassifyPreview> {
    if file_ids.is_empty() {
        return Err(AppError::InvalidInput("file_ids 不能为空".to_string()));
    }

    let safe_root = security::validate(scan_root)?;

    let (files, rules, categories) = {
        let guard = state
            .db
            .lock()
            .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))?;
        let files = FileRepo::get_by_ids(guard.conn(), file_ids)?;
        let rules = RuleRepo::list_enabled(guard.conn())?;
        let categories = CategoryRepo::list_all(guard.conn())?;
        drop(guard);
        (files, rules, categories)
    };

    if files.is_empty() {
        return Err(AppError::InvalidInput("指定文件不存在或已失效".to_string()));
    }

    let items = classifier::generate_plan(&files, &safe_root, &rules, &categories)?;
    let preview_stats = classifier::aggregate_stats(&items);
    let batch_id = uuid::Uuid::new_v4().to_string();

    Ok(ClassifyPreview {
        batch_id,
        items,
        stats: preview_stats,
    })
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::models::FileRecord;
    use crate::db::Database;
    use crate::sidecar::SidecarManager;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    fn make_test_app_state(db_path: &std::path::Path) -> AppState {
        let db = Database::open(db_path).expect("打开测试 DB 失败");
        AppState {
            db: Mutex::new(db),
            sidecar_manager: Mutex::new(SidecarManager::new(
                "/dev/null/sidecar-nonexistent".into(),
            )),
            sidecar_psk: Mutex::new(None),
            sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
        }
    }

    fn seed_one_file(state: &AppState, root: &std::path::Path) -> String {
        let name = "photo.png";
        std::fs::write(root.join(name), b"x").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let rec = FileRecord {
            id: id.clone(),
            path: root.join(name).to_string_lossy().to_string(),
            file_name: name.to_string(),
            file_size: 1,
            content_hash: None,
            category: None,
            is_deleted: false,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
        };
        let guard = state.db.lock().unwrap();
        FileRepo::upsert_batch(guard.conn(), &[rec]).unwrap();
        id
    }

    #[test]
    fn test_classify_preview_returns_plan_and_stats() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        // 内置分类种子：启发式「图片」目标可用
        let db = state.db.lock().map_err(|e| e.to_string())?;
        CategoryRepo::seed_builtin_categories(db.conn()).map_err(|e| e.to_string())?;
        drop(db);

        let id = seed_one_file(&state, root.path());
        let preview =
            classify_preview_inner(&state, &[id], root.path().to_str().ok_or("路径非 UTF-8")?)?;

        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.items[0].category_name.as_deref(), Some("图片"));
        assert_eq!(preview.items[0].rule_source, "heuristic");
        assert_eq!(preview.stats.categorized, 1);
        assert_eq!(preview.stats.pending, 0);
        assert_eq!(preview.batch_id.len(), 36);
        Ok(())
    }

    #[test]
    fn test_classify_preview_empty_ids_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let result =
            classify_preview_inner(&state, &[], root.path().to_str().ok_or("路径非 UTF-8")?);
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
        Ok(())
    }

    #[test]
    fn test_classify_preview_unsafe_scan_root_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let result = classify_preview_inner(&state, &["f1".to_string()], "/System");
        assert!(matches!(result, Err(AppError::UnsafePath(_))));
        Ok(())
    }
}
