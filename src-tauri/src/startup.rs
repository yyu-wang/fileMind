//! 启动期一次性初始化：日志脱敏、数据库打开、内置种子、操作日志链校验、E2E 预置。
//!
//! 除「种子」与「链校验」（失败仅告警、不阻断启动）外，其余步骤失败即 `exit(1)`：
//! 这些是安全与可用性的地基（脱敏正则、DB 可打开），带病启动比直接退出更危险。
//! 调用顺序由 `main` 编排。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use filemind_lib::db::{CategoryRepo, ConfigRepo, Database, OperationRepo, RuleRepo};
use filemind_lib::security::log_redact;

/// 初始化日志与脱敏正则。
///
/// T7.1：自定义 formatter 在「单一出口」统一脱敏，所有日志经 `log_redact::redact`
/// 过滤后再输出（安全 I-03）。formatter 经静态路径调用 `redact`，与 `init` 顺序无关。
///
/// 落盘（2026-09-16）：GUI 双击启动时 stderr 被丢弃，watchdog 的「需要重启 /
/// 重启失败 / CrashLoop」等关键事件必须同时写 `~/.filemind/logs/filemind.log`
/// 才能事后取证，故实际初始化在 `log_file::init_logger`（含 tee + 轮转 + 降级）。
///
/// 正则集合编译失败时直接以非零码退出：此时日志可能明文泄漏敏感信息，不进入无脱敏
/// 运行状态（该失败日志为编译期常量文本，本身不含敏感信息）。
pub fn init_logging() {
    filemind_lib::log_file::init_logger();

    if let Err(e) = log_redact::init() {
        log::error!("致命错误：日志脱敏正则初始化失败: {e}");
        std::process::exit(1);
    }
}

/// 解析数据库路径并打开，返回可共享的连接句柄。
///
/// 打开失败即退出：没有 DB 时后续所有命令都无意义。
pub fn open_database() -> Arc<Mutex<Database>> {
    let db_path = get_db_path();
    match Database::open(&db_path) {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(e) => {
            log::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    }
}

/// 解析数据库文件路径。
///
/// 数据目录优先读 `FILEMIND_DATA_HOME`（与 Python Sidecar 共用同一目录，保证
/// `SQLite` 与 `LanceDB` 落在同一根下）；未设置时回退到 `~/.filemind`（生产默认位置）。
fn get_db_path() -> PathBuf {
    let data_home = std::env::var("FILEMIND_DATA_HOME").unwrap_or_else(|_| {
        // BE-m9：HOME/USERPROFILE 均缺失时用 dirs 解析（unix 走 getpwuid 仍可得
        // 家目录）；彻底解析失败才退平台临时目录并显式告警——数据落临时目录
        // 有丢失风险，不再静默硬编码 /tmp。
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| {
                log::error!(
                    "无法解析用户家目录（HOME/USERPROFILE 均缺失且 dirs 解析失败），数据库回退平台临时目录，数据可能在清理时丢失"
                );
                std::env::temp_dir()
            });
        format!("{}/.filemind", home.display())
    });
    PathBuf::from(data_home).join("filemind.db")
}

/// 取 DB 守卫；启动阶段 Mutex 中毒不可恢复，直接退出。
///
/// 与原 `main` 内的 `lock_db` 闭包同语义：启动期的中毒意味着初始化逻辑已 panic，
/// 继续跑没有意义。`pub` 供同期启动装配复用（`sidecar_setup`）。
pub fn lock_db(database: &Arc<Mutex<Database>>) -> std::sync::MutexGuard<'_, Database> {
    database.lock().unwrap_or_else(|_| {
        log::error!("DB lock poisoned during startup");
        std::process::exit(1);
    })
}

/// T6.5 内置分类种子：保证启发式分类有目标分类可用（幂等，失败不阻断启动）。
pub fn seed_categories(database: &Arc<Mutex<Database>>) {
    let seed_result = {
        let db_guard = lock_db(database);
        CategoryRepo::seed_builtin_categories(db_guard.conn())
    };
    match seed_result {
        Ok(0) => log::info!("内置分类已存在，跳过种子"),
        Ok(n) => log::info!("内置分类种子：新增 {n} 个分类"),
        Err(e) => log::warn!("内置分类种子失败（不影响启动）: {e}"),
    }
}

/// 内置默认规则种子：分类种子之后执行（默认规则外键指向 `builtin-document`）。
///
/// 仅在 rules 表为空时补齐两条默认禁用规则（PDF/文本归档），失败不阻断启动。
pub fn seed_default_rules(database: &Arc<Mutex<Database>>) {
    let seed_rule_result = {
        let db_guard = lock_db(database);
        RuleRepo::seed_default_rules(db_guard.conn())
    };
    match seed_rule_result {
        Ok(0) => log::info!("默认规则已存在或已有自定义规则，跳过种子"),
        Ok(n) => log::info!("默认规则种子：新增 {n} 条规则"),
        Err(e) => log::warn!("默认规则种子失败（不影响启动）: {e}"),
    }
}

/// T9.5 E2E：`FILEMIND_E2E_SKIP_ONBOARDING=1` 时预置配置，让应用直达文件页。
///
/// 安全：仅 debug 构建生效；写入的是本次 E2E 的临时 `SQLite`（`FILEMIND_DATA_HOME`
/// 隔离），不影响真实用户配置；release 不编译此分支。
#[cfg(debug_assertions)]
pub fn apply_e2e_config(database: &Arc<Mutex<Database>>) {
    if !std::env::var("FILEMIND_E2E_SKIP_ONBOARDING").is_ok_and(|v| v == "1") {
        return;
    }
    let mut config = {
        let db_guard = lock_db(database);
        ConfigRepo::get(db_guard.conn()).unwrap_or_default()
    };
    config.onboarding_completed = true;
    config.inference_mode = "local".to_string();
    if let Some(dir) = std::env::var("FILEMIND_E2E_DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        config.data_directory = dir;
    }
    let upsert_result = {
        let db_guard = lock_db(database);
        ConfigRepo::upsert(db_guard.conn(), &config)
    };
    match upsert_result {
        Ok(()) => {
            log::info!("T9.5 E2E：FILEMIND_E2E_SKIP_ONBOARDING=1 已预置 onboarding_completed=true");
        }
        Err(e) => log::error!("T9.5 E2E：预置配置失败: {e}"),
    }
}

/// T3.5：启动时校验操作日志链式哈希完整性，检测到篡改仅告警、不阻断启动。
pub fn verify_operation_chain(database: &Arc<Mutex<Database>>) {
    let verify_result = {
        let db_guard = lock_db(database);
        OperationRepo::verify_chain(db_guard.conn())
    };
    match verify_result {
        Ok(None) => log::info!("操作日志链式哈希校验通过"),
        Ok(Some(break_id)) => {
            log::error!("操作日志链式哈希校验失败，检测到篡改，断裂于记录 {break_id}");
        }
        Err(e) => log::error!("操作日志链式哈希校验出错: {e}"),
    }
}
