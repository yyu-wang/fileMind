//! Sidecar 可执行文件的定位：triple 推断、dev 布局发现与 bundle 布局探测。
//!
//! 全部是纯函数或以环境变量 / 根目录列表为输入的函数，不持有状态、不启动进程：
//! dev 走 `FILEMIND_SIDECAR_BINARY` 覆盖 → `CARGO_MANIFEST_DIR` 回退 → cwd 兜底；
//! bundle 走「主可执行目录 / `resource_dir`」两个根里的 `sidecar/` 子目录。
//! 两条路径都只认 onedir 产物目录里的主可执行文件（P2-2 起的打包形态）。

use crate::error::{AppError, AppResult};
use tauri::Manager as _;

// ---------- 二进制路径解析（dev 模式 / CI 注入） ----------

/// 根据当前编译目标推断 Rust triple 字符串（用于定位 `filemind-sidecar-{triple}` 产物名）。
///
/// 注意：triple 字符串匹配的是 **构建侧** `build-sidecar.sh --target` 的参数。
/// 对于「本机编译本机跑」场景一致；交叉编译环境下由 `FILEMIND_SIDECAR_BINARY` 覆盖，
/// 不会走到该回退。
#[must_use]
pub fn current_target_triple() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin".into(),
        ("macos", "x86_64") => "x86_64-apple-darwin".into(),
        ("windows", "x86_64") => "x86_64-pc-windows-msvc".into(),
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu".into(),
        (os, arch) => format!("{arch}-{os}"),
    }
}

/// dev 模式解析 Sidecar 可执行文件绝对路径（打包模式由 Tauri `PathResolver` 代替）。
///
/// P2-2 起**布局发现**只认 onedir 目录形态（见 [`main_exe_in_dir`]），不再扫描 onefile
/// 单文件产物；但**显式覆盖**（`FILEMIND_SIDECAR_BINARY`）语义是「直接指定要执行的
/// sidecar 可执行文件」，故同时接受目录与可执行文件——CI E2E 与本地 dev 会用 wrapper
/// 脚本（`scripts/e2e-sidecar-wrapper.sh` / `binaries/filemind-sidecar-dev`）注入
/// `python -m app` 启动参数，这是文件而非目录。
///
/// 优先级从高到低：
///
/// 1. `override_env`：调用方读 `FILEMIND_SIDECAR_BINARY` 后传入（绝对/相对都行）；
///    目录 → 定位其中的主可执行；可执行文件 → 直接使用；传 `None` 或空串 → 跳过。
/// 2. 基于 `CARGO_MANIFEST_DIR` 环境变量（cargo 注入，指向 ``<repo>/src-tauri``）：
///    向上回退到 repo 根，然后找 ``filemind/binaries/``：
///    a. ``filemind-sidecar-{triple}/`` 具体架构产物目录（优先生效）
///    b. ``filemind-sidecar/`` 软链接目录（兜底，build-sidecar.sh 创建）
/// 3. 最后回退：``${cwd}/filemind/binaries/filemind-sidecar/``（兼容手工启动场景）。
///
/// 返回：第一个命中的主可执行文件路径（已 `canonicalize`，无相对段）。
///
/// # Errors
///
/// 全部候选目录不存在或无主可执行文件时返回 [`AppError::SidecarUnavailable`]，
/// 错误信息附带候选列表 + 当前 triple 构建建议，便于排障。
pub fn resolve_dev_binary_path(override_env: Option<&str>) -> AppResult<std::path::PathBuf> {
    let mut tried: Vec<String> = Vec::new();

    // ---- 优先级 1：显式覆盖（CI / 调试）----
    if let Some(ov) = override_env.filter(|s| !s.is_empty()) {
        if let Some(p) = resolve_override(ov, &mut tried)? {
            return Ok(p);
        }
    }

    // ---- 优先级 2/3：dev 布局发现（CARGO_MANIFEST_DIR 回退 → cwd 兜底）----
    if let Some(p) = find_in_dev_layout(&mut tried)? {
        return Ok(p);
    }

    // ---- 全部不命中 ----
    Err(AppError::SidecarUnavailable(format!(
        "dev 模式未找到 Sidecar onedir 产物目录（目录内应含主可执行 filemind-sidecar），候选列表:\n  - {}\n\
         建议：1) 先跑 bash scripts/build-sidecar.sh --target {}；2) 或设置 FILEMIND_SIDECAR_BINARY 指向产物目录绝对路径",
        tried.join("\n  - "),
        current_target_triple()
    )))
}

/// 解析显式覆盖（`FILEMIND_SIDECAR_BINARY`）：目录取其中主可执行，可执行文件直接采用。
///
/// 返回语义：
/// - `Ok(Some(p))`：命中（已 canonicalize）
/// - `Ok(None)`：路径不可用 → 交由布局发现继续兜底（保持既有「覆盖不生效不阻塞」语义）
/// - `Err`：命中但 canonicalize 失败（真实故障应暴露）
fn resolve_override(ov: &str, tried: &mut Vec<String>) -> AppResult<Option<std::path::PathBuf>> {
    let p = std::path::PathBuf::from(ov);
    tried.push(format!("(override) {}", p.display()));
    let hit = if p.is_dir() {
        main_exe_in_dir(&p)
    } else if is_existing_file(&p) {
        // wrapper 脚本 / 直接指定的可执行文件
        Some(p)
    } else {
        None
    };
    hit.map_or(Ok(None), |exe| {
        exe.canonicalize()
            .map(Some)
            .map_err(|e| AppError::SidecarUnavailable(format!("canonicalize override 失败: {e}")))
    })
}

/// dev 布局发现：`CARGO_MANIFEST_DIR` 回退到 repo 根的 `filemind/binaries/`，再 cwd 兜底。
///
/// `tried` 累积探测记录（供调用方拼错误信息）；命中返回 `Ok(Some(已 canonicalize 路径))`。
fn find_in_dev_layout(tried: &mut Vec<String>) -> AppResult<Option<std::path::PathBuf>> {
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let src_tauri = std::path::PathBuf::from(manifest_dir);
        // CARGO_MANIFEST_DIR = <repo>/src-tauri → repo 根 = parent()
        if let Some(repo_root) = src_tauri.parent() {
            let binaries_dir = repo_root.join("filemind").join("binaries");
            let triple = current_target_triple();
            // triple 具体目录优先；基础名目录兜底（build-sidecar.sh 在 macOS 建软链接）
            for (tag, dir) in [
                (
                    "cargo-triple",
                    binaries_dir.join(format!("filemind-sidecar-{triple}")),
                ),
                ("cargo-default", binaries_dir.join("filemind-sidecar")),
            ] {
                if let Some(p) = probe_onedir(&dir, tag, tried)? {
                    return Ok(Some(p));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let fallback = cwd
            .join("filemind")
            .join("binaries")
            .join("filemind-sidecar");
        return probe_onedir(&fallback, "cwd", tried);
    }
    tried.push("(cwd) 无法读取 current_dir → 已跳过".to_string());
    Ok(None)
}

/// 在候选 onedir 目录内定位主可执行并 canonicalize；未命中返回 `Ok(None)`。
fn probe_onedir(
    dir: &std::path::Path,
    tag: &str,
    tried: &mut Vec<String>,
) -> AppResult<Option<std::path::PathBuf>> {
    tried.push(format!("({tag}) {}/", dir.display()));
    main_exe_in_dir(dir).map_or(Ok(None), |exe| {
        exe.canonicalize().map(Some).map_err(|e| {
            AppError::SidecarUnavailable(format!("canonicalize {tag} 主可执行失败: {e}"))
        })
    })
}

/// 小 helper：`fs::metadata(p).is_ok_and(|m| m.is_file())`，不用再写多处。
fn is_existing_file(p: &std::path::Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

// ---------- onedir 产物匹配（P2-2） ----------

/// onedir 目录在 bundle 内的落地子目录名。
///
/// 与 `src-tauri/tauri.conf.json` 中 `bundle.resources` map 的目标值保持一致：
/// `{"../filemind/binaries/filemind-sidecar/": "sidecar"}` → 主可执行落在
/// `$RESOURCE/sidecar/filemind-sidecar`（macOS）/ `<install>/sidecar/...`（Windows）。
///
/// `pub(super)`：由 `manager/mod.rs` 同可见性地再导出，测试模块的通配导入要用。
pub(super) const BUNDLE_SIDECAR_SUBDIR: &str = "sidecar";

/// onedir 主可执行的**基础名**（无 triple 后缀；Windows 带 `.exe`）。
///
/// `pub(super)`：`impl Default for SidecarManager`（在 `manager/mod.rs`）拼默认 dev 路径时要用。
#[must_use]
pub(super) const fn main_exe_base_name() -> &'static str {
    if cfg!(windows) {
        "filemind-sidecar.exe"
    } else {
        "filemind-sidecar"
    }
}

/// onedir 主可执行文件的候选文件名（不含目录），按优先级排列。
///
/// P2-2 后产物是目录（`filemind-sidecar[.exe]` + `_internal/`）。构建侧命名可能保留
/// triple 后缀或退化为基础名；与 `build-sidecar.sh` 的产物命名保持一致。
#[must_use]
fn resolve_main_exe_names() -> Vec<String> {
    let triple = current_target_triple();
    let mut names = Vec::new();
    if cfg!(windows) {
        names.push(format!("filemind-sidecar-{triple}.exe"));
    }
    names.push(format!("filemind-sidecar-{triple}"));
    names.push(main_exe_base_name().to_string());
    names
}

/// 在给定目录内定位 onedir 主可执行文件（纯逻辑）。
///
/// 只认「目录内的真实文件」：目录本身不是可执行产物，真正要启动的是其中的主程序，
/// 其同级的 `_internal/` 由 `PyInstaller` bootloader 自行定位。
///
/// 声明为 `pub` 而非 `pub(crate)`：所在模块是私有的，`pub(crate)` 已被 clippy 判为冗余；
/// 实际可见范围仍限于本模块树，对外的转发由 `manager/mod.rs` 控制。
#[must_use]
pub fn main_exe_in_dir(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    resolve_main_exe_names()
        .into_iter()
        .map(|name| dir.join(name))
        .find(|c| is_existing_file(c))
}

/// bundle 模式下，在多个「根目录」中依次探测 onedir 主可执行文件。
///
/// 纯函数：按给定根目录顺序，每个根目录内依次尝试
/// 1. `<root>/sidecar/`（Tauri `bundle.resources` 的 map 目标落地位置，P2-2 主路径）
/// 2. `<root>/`（未嵌套的 onedir 目录，便于手工摆放 / 调试）
///
/// 首个命中即返回（已 canonicalize）。
///
/// Tauri v2 实测行为（2026-09-04）：`externalBin` 产物落在主可执行同目录；P2-2 改用
/// `bundle.resources` 后落在 `resource_dir()/sidecar/`（macOS `Contents/Resources/sidecar/`）。
/// 因此调用方同时传「主可执行目录」与 `resource_dir` 两个根，任一命中即可。
///
/// # Errors
///
/// 全部根目录 × 全部候选均不存在 / 非文件 → 返回 [`AppError::SidecarUnavailable`]，
/// 附完整探测列表，便于现场排障（如打包脚本漏拷产物目录）。
pub fn resolve_bundle_from_roots(roots: &[std::path::PathBuf]) -> AppResult<std::path::PathBuf> {
    let mut tried: Vec<String> = Vec::new();
    for root in roots {
        for sub in [Some(BUNDLE_SIDECAR_SUBDIR), None] {
            let dir = sub.map_or_else(|| root.clone(), |s| root.join(s));
            tried.push(format!("{}/", dir.display()));
            if let Some(exe) = main_exe_in_dir(&dir) {
                return exe.canonicalize().map_err(|e| {
                    AppError::SidecarUnavailable(format!(
                        "Sidecar 命中 bundle 候选 {} 但 canonicalize 失败: {e}",
                        exe.display()
                    ))
                });
            }
        }
    }

    Err(AppError::SidecarUnavailable(format!(
        "Sidecar 主可执行在 bundle 根目录下未找到（探测根目录 {} 个，候选路径 {} 条）:\n  {}\n\
         请确认 tauri.conf.json bundle.resources 已配置目录映射，且 build-sidecar.sh 产物在构建前生成",
        roots.len(),
        tried.len(),
        tried.join("\n  ")
    )))
}

/// bundle 模式下，基于给定的单一「resources 根目录」解析 onedir 主可执行文件。
///
/// 纯函数：兼容旧单测契约（直接传 `tempdir` 验证拼接与错误文案），内部委托
/// [`resolve_bundle_from_roots`]。生产路径走 [`resolve_bundle_binary_path`]
/// （多根目录探测，含主可执行文件目录）。
///
/// # Errors
///
/// 全部候选不存在 / 非文件 → 返回 [`AppError::SidecarUnavailable`]，附候选列表。
pub fn resolve_bundle_from_resources(
    resources_root: &std::path::Path,
) -> AppResult<std::path::PathBuf> {
    resolve_bundle_from_roots(&[resources_root.to_path_buf()])
}

/// bundle 模式下，解析打包态 Sidecar onedir 主可执行文件的绝对路径。
///
/// 解析到的路径即传给 [`SidecarManager::new`] 启动；本函数仅做路径定位，不含进程启动。
///
/// 根目录收集顺序（P2-2：产物经 `bundle.resources` 携带）：
/// 1. **主可执行文件所在目录**（macOS `Contents/MacOS/`，Windows 安装根目录）——
///    兼容「onedir 目录被摆在主可执行旁」的场景；
/// 2. `resource_dir()`（macOS `Contents/Resources/`，Windows 安装根目录）——
///    `bundle.resources` 的实际落地位置，P2-2 主命中路径
///    （实际主可执行在 `resource_dir()/sidecar/`，见 [`BUNDLE_SIDECAR_SUBDIR`]）。
///
/// 纯逻辑在 [`resolve_bundle_from_roots`]，此处仅负责收集根目录，单测无需 Mock Tauri。
///
/// # Errors
///
/// - 主可执行文件目录无法解析（非 bundle 环境？）与 `resource_dir()` 查询失败时记 warn，
///   不阻断（仍有另一根目录可探）
/// - 全部根目录候选均不命中（详情见 [`resolve_bundle_from_roots`]）
pub fn resolve_bundle_binary_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    match app.path().resource_dir() {
        Ok(dir) => roots.push(dir),
        Err(e) => log::warn!("Tauri resource_dir 查询失败（作为兜底根目录跳过）: {e}"),
    }
    resolve_bundle_from_roots(&roots)
}
