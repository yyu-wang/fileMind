//! Sidecar 进程生命周期管理：启动、握手、健康探测、崩溃重启与优雅停止。
//!
//! 启动流程：
//! 1. 生成 PSK（32 字节随机）→ 通过 stdin 注入 Sidecar
//! 2. 轮询 /health 直到 Sidecar 就绪
//! 3. POST /handshake 完成身份验证
//! 4. 返回 PSK，由调用方存入 `AppState` 供后续请求签名
//!
//! 运行期：
//! - 由外部（`main.rs`）`tokio::spawn` 调 `health_watchdog_step` 循环探活
//! - 连续 3 次 /health 失败或 `Child::try_wait` 返回已退出 → `restart()`
//! - 重启采用指数退避 1s→2s→4s→8s 上限；1 分钟内 10 次 → `SidecarCrashLoop` 暂停自动恢复
//!
//! 停止：
//! - `stop_graceful()` 先请求 POST /shutdown，等 3s 看 Sidecar 自退
//! - 仍存活则 `kill()` + `wait()` 兜底；总耗时 ≤ 5s
//! - `stopped` 标志位 + 内部 ``OnceCell`` 保证 stop 只真实执行一次（`Drop` 兜底不再重复杀）
//!
//! 模块划分（原单文件 1111 行按职责拆分，各文件 < 300 行，见 `rules/complexity.md`）：
//!   - `start`    spawn + PSK 注入、就绪轮询、握手与失败回滚
//!   - `watchdog` `/health` 探活、指数退避、重启与崩溃循环窗口
//!   - `shutdown` 优雅停止、硬杀、`Drop` 兜底
//!   - `orphan_cleanup` 启动前清理上次残留的孤儿进程（身份匹配 + 孤儿判定 + 终止）
//!   - `paths`    triple 推断与 dev / bundle 布局解析（纯函数）
//!
//! 本文件保留类型与门面。`SidecarManager` 的状态必须定义在这里：测试模块
//! （`manager_tests`，本模块的后代）直接读写其私有字段，字段可见性依赖定义位置。
//! 方法散在子模块的 `impl SidecarManager` 块中，对调用方而言仍是同一个类型的方法集合。

use std::collections::VecDeque;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod orphan_cleanup;
mod paths;
mod shutdown;
mod start;
mod watchdog;

// —— 对外再导出 ——
// 子模块承载实现，类型与自由函数在这里再导出，使 `sidecar::manager::X` 的既有路径
// （`sidecar/mod.rs` 的再导出、`bootstrap.rs` 的直接引用、测试模块的通配导入）保持不变。
// 方法无需再导出：`impl` 块在 crate 内、方法本身 `pub`，即自动对调用方可见。
pub use orphan_cleanup::cleanup_orphan_sidecar;
pub use paths::{
    current_target_triple, resolve_bundle_binary_path, resolve_bundle_from_resources,
    resolve_bundle_from_roots, resolve_dev_binary_path,
};
pub use watchdog::WatchdogAction;

// crate 内可见项：`bootstrap.rs` 用的是 `main_exe_in_dir`，原先在本文件里，拆分后
// 按原可见性转发，调用方无需改动。只被测试用到的 `matches_sidecar_comm` 与
// `BUNDLE_SIDECAR_SUBDIR` 不做转发——测试模块是 `manager` 的后代，直接按路径引入即可，
// 免得留下「只为测试存在的再导出」（与 `commands/file_ops` 拆分的处理一致）。
pub(crate) use paths::main_exe_in_dir;

/// Sidecar 固定监听端口（本机回环）。
pub const SIDECAR_PORT: u16 = 8765;
/// Sidecar 就绪轮询最大尝试次数。
/// 打包态 Sidecar 现包含 lancedb / numpy / pyarrow 等重依赖，冷启动主要由
/// `PyInstaller` onefile 解压主导（P2-1 惰性导入后本机实测 21s 冷 / 16s 热盘）。
/// 保留 60s 窗口（600 × 100ms）作为慢盘 / 首次解压的余量，避免握手超时。
const MAX_READY_ATTEMPTS: u32 = 600;
/// 每次就绪轮询间隔（毫秒）。
const READY_POLL_INTERVAL_MS: u64 = 100;
/// 连续 /health 失败阈值：达到后认为 Sidecar 挂了，触发重启。
///
/// 30 次（watchdog tick=1s，每次失败还叠加最高 3s 的健康请求超时，实际容忍窗口 ≥30s）：
/// **必须容忍长时阻塞**。打包版首次加载 rerank 模型（`import torch` + `CrossEncoder`
/// 冷构造）期间事件循环可能数十秒无法应答 /health——本机实测冷加载 21.6s，旧阈值
/// 3（≈3s）必然误判为挂死并杀进程重启。实机回归（2026-09-16）：用户首问 → 加载
/// rerank → 连续失败 3 次被判挂死 → 杀进程 → 重启失败转 Failed → 8765 再无监听，
/// 此后所有 AI 请求都是「Sidecar POST 失败: error sending request」，只能手动重试。
///
/// 真正的「接受连接但不响应」仍会被检出并重启，只是从 3s 放宽到 30 次。
const HEALTH_FAIL_THRESHOLD: u32 = 30;
/// 指数退避初始值（毫秒）：首次重启失败后下一次等待 1s。
const RESTART_BACKOFF_BASE_MS: u64 = 1000;
/// 指数退避上限（毫秒）：无论失败多少次，最多等 8s 再试。
const RESTART_BACKOFF_CAP_MS: u64 = 8000;
/// `CrashLoop` 观察窗口（秒）：此窗口内重启次数超过阈值则暂停自动恢复。
/// 直接 `u32`：对应 `AppError::SidecarCrashLoop.window_secs` 字段类型，
/// 使用处 `Duration::from_secs` 接受 `u64`，再转一次不丢。
const CRASH_LOOP_WINDOW_SECS: u32 = 60;
/// `CrashLoop` 阈值：窗口内最大允许的重启次数。
const CRASH_LOOP_MAX_RESTARTS: u32 = 10;
/// 优雅关闭：`POST /shutdown` 后等待 Sidecar 自退的最大时长（秒）。
const GRACEFUL_SELF_EXIT_SECS: u64 = 3;
/// 优雅关闭：`kill` 之后 `wait` 兜底 + 余量总超时（秒），总 `DoD` ≤ 5s。
const GRACEFUL_TOTAL_TIMEOUT_SECS: u64 = 5;

/// 云端模式需要注入 Sidecar 进程的 env（T7.4 代理接线）。
///
/// 由 Rust 在启动 Sidecar 前设置；Sidecar 侧 E8 Provider 据此调用
/// `FILEMIND_CLOUD_PROXY_URL` 代理并携带 `FILEMIND_CLOUD_PROXY_TOKEN` 鉴权头，
/// `FILEMIND_CLOUD_MASKING` 触发 T7.2 云端脱敏。
#[derive(Clone)]
pub struct CloudSidecarEnv {
    /// Rust 云端代理地址（`http://127.0.0.1:{CLOUD_PROXY_PORT}`）。
    pub proxy_url: String,
    /// 代理调用方共享 token（Sidecar 请求时放 `X-FileMind-Token`）。
    pub proxy_token: String,
    /// 是否启用云端数据脱敏（T7.2，`inference_mode=cloud` 时为真）。
    pub masking_on: bool,
    /// P-07：当前激活的云提供商 slug（`app_config.active_cloud_provider`），
    /// Sidecar 通过 env `FILEMIND_ACTIVE_CLOUD_PROVIDER` 读取后，
    /// `GenericCloudProvider` 用它拼 Rust 云端代理路由尾段。空串表示未指定
    /// （`ProviderFactory` 回落内置前缀匹配 + 本地 `Ollama`）。
    pub active_cloud_provider: String,
}

/// 本地生成后端需要注入 Sidecar 进程的 env（T3b）。
///
/// 由 Rust 在启动 Sidecar 前从 `app_config` 读取：Sidecar 不读 SQLite，本地生成走
/// Ollama 还是内置 llama.cpp 引擎只能这样送进去（同 `CloudSidecarEnv` 的原因）。
/// 未安装 Ollama 的部署机器靠 `builtin` 完成问答。
#[derive(Clone)]
pub struct LocalLlmSidecarEnv {
    /// 用户配置的本地生成后端（`ollama` / `builtin`）。
    pub backend: String,
    /// 内置后端的 GGUF 模型标识（模型目录名，对应 Python 侧注册表）。
    pub model: String,
}

/// Sidecar 进程管理器：持有子进程句柄，析构时自动停止。
pub struct SidecarManager {
    /// Sidecar 二进制绝对路径，由调用方在构造时显式注入。
    ///
    /// dev 模式：`resolve_dev_binary_path()` 解析自 env / repo 相对路径；
    /// bundle 模式：Tauri v2 `app.path().resolve(...)` 解析自 `MacOS` / 安装目录。
    ///
    /// 字段名加下划线后缀避免与同名访问器方法 [`SidecarManager::binary_path`] 冲突
    /// （字段私有，仅内部实现访问；对外一律通过访问器）。
    binary_path_: std::path::PathBuf,
    /// 子进程句柄（未启动时为 `None`）。
    process: Option<Child>,
    /// Sidecar 监听端口。
    port: u16,
    /// 云端模式 env 注入（`None` = 本地模式，不注入；重启后自动保持）。
    cloud_env: Option<CloudSidecarEnv>,
    /// 本地生成后端 env 注入（`None` = 不注入，Sidecar 用其默认值 `ollama`）。
    local_llm_env: Option<LocalLlmSidecarEnv>,
    /// 当前 Sidecar 握手后的 PSK（`restart` 后替换为新 PSK）。
    psk: Option<Vec<u8>>,
    /// 最近重启时间戳队列：用于 `CrashLoop` 窗口阈值统计。
    recent_restarts: VecDeque<Instant>,
    /// 重启失败连续次数：控制指数退避；成功握手后清零。
    consecutive_failures: u32,
    /// 最近一次 /health 失败累计次数；成功即清零。
    recent_health_fails: u32,
    /// 是否已执行过真实停止动作：`stop_graceful` 首次 `set`，
    /// `Drop` 中检查为 `true` 则跳过，避免 `on_exit` 主路径 + `Drop` 重复 `kill`。
    stopped: AtomicBool,
}

impl SidecarManager {
    /// 用调用方解析好的 Sidecar 二进制绝对路径创建管理器（尚未启动进程）。
    ///
    /// 启动语义：本函数不校验 `binary_path` 是否存在，若路径无效，会在
    /// [`SidecarManager::start`] 的 `Command::spawn` 阶段返回
    /// [`AppError::SidecarUnavailable`]（附路径信息，便于排障）。
    #[must_use]
    pub const fn new(binary_path: std::path::PathBuf) -> Self {
        Self {
            binary_path_: binary_path,
            process: None,
            port: SIDECAR_PORT,
            cloud_env: None,
            local_llm_env: None,
            psk: None,
            recent_restarts: VecDeque::new(),
            consecutive_failures: 0,
            recent_health_fails: 0,
            stopped: AtomicBool::new(false),
        }
    }

    /// 当前 Sidecar 二进制路径（供排障面板 / 日志展示）。
    #[must_use]
    pub fn binary_path(&self) -> &std::path::Path {
        &self.binary_path_
    }

    /// 测试场景：拿到构造时内部 `binary_path` 引用（同 crate 可见，避免对外公开字段）。
    #[cfg(test)]
    pub(crate) fn binary_path_inner(&self) -> &std::path::Path {
        &self.binary_path_
    }

    /// 是否已停止（一次性置位）。
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// 当前 PSK：握手成功后为 `Some`，重启后会换成新 PSK。
    #[must_use]
    pub fn psk(&self) -> Option<&[u8]> {
        self.psk.as_deref()
    }

    /// 当前子进程 PID（未启动/已退出后 Zombie 等待被 wait 前仍返回旧 pid）。
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(Child::id)
    }

    /// 设置云端模式 env 注入（须在 `start` 之前调用；重启自动沿用）。
    pub fn set_cloud_env(&mut self, cloud_env: CloudSidecarEnv) {
        self.cloud_env = Some(cloud_env);
    }

    /// 当前云端模式 env 配置（未设置时为 `None`）。
    ///
    /// 供 setup 在「用 bundle 路径重建 SidecarManager」时把 main 阶段解析好的
    /// 云端 env 克隆到新管理器，避免打包态云端模式丢配置（T7.4）。
    #[must_use]
    pub const fn cloud_env(&self) -> Option<&CloudSidecarEnv> {
        self.cloud_env.as_ref()
    }

    /// 设置本地生成后端 env 注入（须在 `start` 之前调用；重启自动沿用）。
    pub fn set_local_llm_env(&mut self, env: LocalLlmSidecarEnv) {
        self.local_llm_env = Some(env);
    }

    /// 当前本地生成后端 env 配置（未设置时为 `None`）。
    ///
    /// 同 [`Self::cloud_env`]：setup 重建管理器时需要克隆过去，否则打包态会丢配置。
    #[must_use]
    pub const fn local_llm_env(&self) -> Option<&LocalLlmSidecarEnv> {
        self.local_llm_env.as_ref()
    }
}

impl Default for SidecarManager {
    fn default() -> Self {
        // Default 仅用于 Mutex::new(Default::default()) 类型占位或单测；
        // 真实二进制运行前（main/setup）会被具体解析后的路径覆盖。
        // P2-2：产物是 onedir 目录 ``binaries/filemind-sidecar/``，主可执行在目录内
        // （文件名按平台），故拼接 ``.../binaries/filemind-sidecar/filemind-sidecar[.exe]``，
        // 即便文件不存在，也保证 binary_path() 语义对应约定的 dev 产物位置。
        let dev_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("filemind")
            .join("binaries")
            .join("filemind-sidecar");
        Self::new(dev_dir.join(paths::main_exe_base_name()))
    }
}

// 测试模块树见 manager_tests/（原 899 行单文件按关注点拆分）；必须写 #[path]——
// 本文件是 manager/mod.rs，`mod x;` 会去找 manager/manager_tests/；测试目录仍在
// sidecar/ 下，故用 ../ 指回上一级（与 db/file_repo/mod.rs 引用 ../file_repo_tests.rs 同理）。
#[cfg(test)]
#[path = "../manager_tests/mod.rs"]
mod manager_tests;

// lint fix notes: doc_markdown (GenericCloudProvider / ProviderFactory / Ollama 反引号)
