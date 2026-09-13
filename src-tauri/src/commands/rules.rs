//! 规则编辑命令：规则的增删改查与优先级拖拽排序。
//!
//! 规则引擎消费端见 `services/classifier`（`match_rule` 支持 `extension`/`path_keyword`/`regex`，
//! `magic_number`/`size` 属 E4 补充，后端暂不匹配，前端表单以禁用占位呈现）。

use tauri::State;

use crate::db::models::{Category, Rule};
use crate::db::{CategoryRepo, RuleRepo};
use crate::error::{AppError, AppResult};
use crate::AppState;

/// 查询全部规则（含禁用项），按优先级降序（数字越大越先匹配）。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn list_rules(state: State<'_, AppState>) -> Result<Vec<Rule>, String> {
    list_rules_inner(&state).map_err(|e| e.to_string())
}

/// 新建（id 为空时生成 UUID）或更新规则，返回落库后的完整规则（含 DB 生成时间戳）。
///
/// # Errors
///
/// 数据库锁中毒、写入或回读失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn upsert_rule(state: State<'_, AppState>, rule: Rule) -> Result<Rule, String> {
    upsert_rule_inner(&state, rule).map_err(|e| e.to_string())
}

/// 删除规则。
///
/// # Errors
///
/// 数据库锁中毒或删除失败（含规则不存在）时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn delete_rule(state: State<'_, AppState>, id: String) -> Result<(), String> {
    delete_rule_inner(&state, &id).map_err(|e| e.to_string())
}

/// 按拖拽后的顺序重排规则优先级，返回重排后的完整列表。
///
/// # Errors
///
/// 数据库锁中毒或排序失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn reorder_rules(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> Result<Vec<Rule>, String> {
    reorder_rules_inner(&state, &ordered_ids).map_err(|e| e.to_string())
}

/// 查询全部分类（规则编辑页目标分类下拉用，只读）。
///
/// # Errors
///
/// 数据库锁中毒或查询失败时返回错误。
#[tauri::command(async)]
#[specta::specta]
pub fn list_categories(state: State<'_, AppState>) -> Result<Vec<Category>, String> {
    list_categories_inner(&state).map_err(|e| e.to_string())
}

/// 锁定数据库句柄，锁中毒映射为 `InvalidInput`（与 classify 命令一致）。
fn lock_db(
    db: &std::sync::Mutex<crate::db::Database>,
) -> AppResult<std::sync::MutexGuard<'_, crate::db::Database>> {
    db.lock()
        .map_err(|e| AppError::InvalidInput(format!("DB 锁中毒: {e}")))
}

fn list_rules_inner(state: &AppState) -> AppResult<Vec<Rule>> {
    let guard = lock_db(&state.db)?;
    RuleRepo::list_all(guard.conn())
}

fn upsert_rule_inner(state: &AppState, mut rule: Rule) -> AppResult<Rule> {
    // 新建规则：前端传空 id，此处生成 UUID4 作为主键
    if rule.id.trim().is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    let guard = lock_db(&state.db)?;
    RuleRepo::upsert(guard.conn(), &rule)?;
    RuleRepo::get_by_id(guard.conn(), &rule.id)
}

fn delete_rule_inner(state: &AppState, id: &str) -> AppResult<()> {
    let guard = lock_db(&state.db)?;
    RuleRepo::delete(guard.conn(), id)
}

fn reorder_rules_inner(state: &AppState, ordered_ids: &[String]) -> AppResult<Vec<Rule>> {
    let guard = lock_db(&state.db)?;
    RuleRepo::reorder(guard.conn(), ordered_ids)?;
    RuleRepo::list_all(guard.conn())
}

fn list_categories_inner(state: &AppState) -> AppResult<Vec<Category>> {
    let guard = lock_db(&state.db)?;
    CategoryRepo::list_all(guard.conn())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sidecar::SidecarManager;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    fn make_test_app_state(db_path: &std::path::Path) -> AppState {
        let db = Database::open(db_path).expect("打开测试 DB 失败");
        AppState {
            db: std::sync::Arc::new(Mutex::new(db)),
            sidecar_manager: Mutex::new(SidecarManager::new(
                "/dev/null/sidecar-nonexistent".into(),
            )),
            sidecar_psk: Mutex::new(None),
            sidecar_binary: Mutex::new("/dev/null/sidecar-nonexistent".into()),
            request_seq: AtomicU64::new(0),
            sidecar_restart_count: AtomicU64::new(0),
            sidecar_status: Mutex::new(crate::SidecarStatus::Starting),
        }
    }

    fn mk_rule(id: &str, name: &str, priority: i64) -> Rule {
        Rule {
            id: id.to_string(),
            name: name.to_string(),
            rule_type: "extension".to_string(),
            pattern: "pdf,doc".to_string(),
            target_category: None,
            priority,
            is_enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn upsert_generates_id_and_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let saved = upsert_rule_inner(&state, mk_rule("", "PDF 规则", 100))?;
        assert_eq!(saved.id.len(), 36); // uuid4
        assert_eq!(saved.name, "PDF 规则");
        assert_eq!(saved.pattern, "pdf,doc");
        assert!(!saved.created_at.is_empty()); // DB datetime('now') 回读

        let all = list_rules_inner(&state)?;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, saved.id);
        Ok(())
    }

    #[test]
    fn reorder_updates_priority_desc() -> Result<(), Box<dyn std::error::Error>> {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let guard = lock_db(&state.db)?;
        RuleRepo::upsert(guard.conn(), &mk_rule("r1", "一", 100))?;
        RuleRepo::upsert(guard.conn(), &mk_rule("r2", "二", 50))?;
        drop(guard);

        let reordered = reorder_rules_inner(&state, &["r2".to_string(), "r1".to_string()])?;
        assert_eq!(reordered[0].id, "r2");
        assert_eq!(reordered[1].id, "r1");
        Ok(())
    }

    #[test]
    fn list_categories_returns_seeded() -> Result<(), Box<dyn std::error::Error>> {
        let tmp_db = tempfile::NamedTempFile::new()?;
        let state = make_test_app_state(tmp_db.path());

        let guard = lock_db(&state.db)?;
        CategoryRepo::seed_builtin_categories(guard.conn())?;
        drop(guard);

        let categories = list_categories_inner(&state)?;
        assert!(!categories.is_empty());
        Ok(())
    }
}
