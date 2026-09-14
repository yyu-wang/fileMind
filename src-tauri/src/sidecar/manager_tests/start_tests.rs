//! 启动相关测试：二进制缺失/占位二进制的报错信息、dev 目录默认值、
//! bundle 布局解析（含真实 macOS `.app` 结构）与候选路径报错。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达（生产代码严格禁止）。

use super::super::*;
use super::support::*;

#[test]
fn test_new_holds_given_path() {
    let p = stub_binary();
    let m = SidecarManager::new(p.clone());
    assert_eq!(m.binary_path_inner(), p.as_path());
    assert_eq!(m.binary_path(), p.as_path());
}

#[test]
fn test_start_fails_on_nonexistent_binary_with_nice_message() {
    // 给一个不存在的路径 → start() 报 SidecarUnavailable 且携带路径信息
    let mut m = SidecarManager::new(PathBuf::from("/does/not/exist/filemind-sidecar-vx"));
    match m.start() {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains("/does/not/exist/filemind-sidecar-vx"),
                "错误信息应包含路径，实际: {msg}"
            );
        }
        other => panic!("start 对不存在的 binary 应返回 SidecarUnavailable，实际: {other:?}"),
    }
}

#[test]
fn test_default_sidecar_uses_dev_binaries_dir() {
    // Default binary_path 纯基于编译时 env!("CARGO_MANIFEST_DIR") 常量拼接，
    // 不读运行时 env，不需要 EnvGuard。P2-2：产物是 onedir 目录，主可执行在目录内，
    // 拼接结果应为：
    //   ${CARGO_MANIFEST_DIR}/../filemind/binaries/filemind-sidecar/filemind-sidecar[.exe]；
    // 不 canonicalize（真实产物可能尚未构建），直接比较逻辑路径。
    let m = SidecarManager::default();
    let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("filemind")
        .join("binaries")
        .join("filemind-sidecar")
        .join(main_exe_name());
    assert_eq!(m.binary_path_inner(), expected.as_path());
    assert_eq!(m.binary_path(), expected.as_path());
}

#[test]
fn test_start_uses_placeholder_binary_gives_nice_error() {
    // 覆盖「管理器 binary_path 字段值真的通过 Command::new 执行层」：
    // 传一个明确不存在的 `filemind/binaries/filemind-sidecar` 风格路径，
    // start() 会返回 AppError::SidecarUnavailable，且错误信息中包含该路径。
    // （注意：不能用 SidecarManager::default() —— 真实开发机上 dev 产物可能已存在，
    //  会真的启动进程并握手成功；此处用固定不存在路径，验证路径透传即可。）
    let bogus = PathBuf::from("/tmp/filemind-tests-notexist-bogus")
        .join("filemind")
        .join("binaries")
        .join("filemind-sidecar");
    let mut m = SidecarManager::new(bogus.clone());
    match m.start() {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains(bogus.to_str().unwrap()),
                "start() 错误信息应包含传入的路径，实际: {msg}"
            );
            assert!(
                msg.contains("filemind/binaries/filemind-sidecar"),
                "路径透传到错误信息时应保留 filemind/binaries/ 上下文，实际: {msg}"
            );
        }
        other => {
            panic!("SidecarManager::new(不存在).start() 应返回 SidecarUnavailable，实际: {other:?}")
        }
    }
}

#[test]
fn test_resolve_bundle_prefers_sidecar_subdir() {
    // P2-2 主路径：Tauri `bundle.resources` 把 onedir 目录拷到 `<root>/sidecar/`
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let sidecar_dir = tmp.path().join(BUNDLE_SIDECAR_SUBDIR);
    std::fs::create_dir_all(&sidecar_dir).expect("mkdir sidecar 不应失败");
    let exe = sidecar_dir.join(main_exe_name());
    std::fs::write(&exe, b"fake-exe").expect("写主可执行失败");

    let got = resolve_bundle_from_resources(tmp.path()).expect("<root>/sidecar 命中应解析成功");
    assert_eq!(got, exe.canonicalize().unwrap());
}

#[test]
fn test_resolve_bundle_accepts_flat_onedir_dir() {
    // 兜底路径：onedir 目录直接摆在根目录下（无 sidecar 子目录嵌套）
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let exe = tmp.path().join(main_exe_name());
    std::fs::write(&exe, b"fake-flat").expect("写主可执行失败");

    let got = resolve_bundle_from_resources(tmp.path()).expect("根目录直放主可执行应解析成功");
    assert_eq!(got, exe.canonicalize().unwrap());
}

/// P2-2 打包态集成契约：真实 macOS `.app` 布局下必须命中 bundle 侧车。
///
/// 复刻 `FileMind.app/Contents/` 结构（`MacOS/` 放主程序、`Resources/sidecar/` 放
/// onedir 产物），按 `resolve_bundle_binary_path` 的根目录顺序（可执行目录 →
/// `resource_dir`）调用纯函数，断言命中 `Resources/sidecar/filemind-sidecar`。
/// 这是 P2-2 由 externalBin 改 bundle.resources 后最关键的一处路径契约，
/// 离线可测（无需真实打包与 GUI 启动）。
#[test]
fn test_resolve_bundle_hits_real_macos_app_layout() {
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    let app = tmp.path().join("FileMind.app").join("Contents");
    let macos_dir = app.join("MacOS");
    let resources_dir = app.join("Resources");
    let sidecar_dir = resources_dir.join(BUNDLE_SIDECAR_SUBDIR);
    std::fs::create_dir_all(&macos_dir).expect("mkdir MacOS 失败");
    std::fs::create_dir_all(&sidecar_dir).expect("mkdir Resources/sidecar 失败");
    // 主程序同目录无 sidecar（旧 externalBin 布局已不再使用）
    std::fs::write(macos_dir.join("filemind"), b"app-exe").expect("写主程序失败");
    let exe = sidecar_dir.join(main_exe_name());
    std::fs::write(&exe, b"sidecar-exe").expect("写 sidecar 主可执行失败");

    let got = resolve_bundle_from_roots(&[macos_dir, resources_dir])
        .expect("真实 .app 布局应命中 bundle 侧车");
    assert_eq!(got, exe.canonicalize().unwrap());
}

#[test]
fn test_resolve_bundle_missing_reports_candidates() {
    let tmp = tempfile::tempdir().expect("tempdir 不应失败");
    match resolve_bundle_from_resources(tmp.path()) {
        Err(AppError::SidecarUnavailable(msg)) => {
            assert!(
                msg.contains(BUNDLE_SIDECAR_SUBDIR),
                "错误信息应列出 sidecar 子目录候选，实际: {msg}"
            );
            assert!(
                msg.contains("bundle.resources"),
                "错误信息应提示 bundle.resources 配置，实际: {msg}"
            );
            assert!(msg.contains("候选"), "错误信息应包含候选提示，实际: {msg}");
        }
        other => panic!("候选均不存在时应返回 SidecarUnavailable，实际: {other:?}"),
    }
}
