//! 探测与孤儿进程清理测试：握手探测超时、启动失败回收、
//! `cleanup_orphan_sidecar` 的名称匹配（含 Linux `comm` 15 字符截断）与不误杀。
//!
//! 本文件自带进程级工具（监听端口/杀监听者/ps 快照），仅这一组用例需要真实进程。

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_fun_call)]
// 测试代码允许：unwrap / panic 是测试失败的最直观表达（生产代码严格禁止）。

use super::super::*;
use super::support::*;

// ---------- BE-C4：探活 client 必须有超时 ----------

/// 挂死端点（接受 TCP 连接但永不响应）上，探活 client 必须在 3s 超时内
/// 快速失败，而不是无限挂起——否则 watchdog 持锁路径会被一起拖死。
#[test]
fn test_probe_client_times_out_on_hung_endpoint() {
    // 只 bind 不 accept：内核 backlog 会完成 TCP 握手，HTTP 请求发出后
    // 永远等不到响应，精确复现「接受连接但不响应」的挂死形态
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 随机端口失败");
    let port = listener.local_addr().expect("local_addr 失败").port();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime 构建失败");
    let started = std::time::Instant::now();
    let result = rt.block_on(async {
        proxy::probe_client()
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
    });

    assert!(result.is_err(), "挂死端点应因超时返回 Err");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "应在 3s 超时 + 余量内返回，实际耗时 {:?}",
        started.elapsed()
    );
}

// ---------- BE-M3 / BE-M6：启动失败清理与孤儿清理 ----------

/// `abort_failed_start` 原语：杀掉未验证子进程 + 清 PSK，且不得置 stopped
/// （否则 watchdog 判定「已主动关闭」永久退出，重启退避循环失效）。
#[cfg(unix)]
#[test]
fn test_abort_failed_start_kills_process_and_clears_psk() {
    let mut m = SidecarManager::new(stub_binary());
    // 手动塞入真实存活子进程 + 未验证 PSK，模拟 start() 成功后握手失败的状态
    m.process = Some(
        std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep 失败"),
    );
    let pid = m.pid().expect("启动后应有 pid");
    m.psk = Some(vec![1u8, 2, 3]);

    m.abort_failed_start();

    assert!(m.psk.is_none(), "未验证 PSK 必须清除");
    assert!(m.process.is_none(), "子进程句柄必须清空");
    assert!(!m.is_stopped(), "不得置 stopped 标志");
    // 子进程确实被杀（stop_hard 已 wait 回收，ps 查不到）
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .output()
        .expect("ps 执行失败");
    assert!(!out.status.success(), "子进程 pid={pid} 应已被杀");
}

/// 工具缺失时跳过孤儿清理测试（lsof/nc/sh 任一不可用即返回）。
#[cfg(unix)]
fn orphan_test_tools_ready() -> bool {
    ["lsof", "nc", "sh"]
        .iter()
        .all(|t| std::process::Command::new("which").arg(t).output().is_ok())
}

/// 轮询直到端口出现监听进程（返回 pid），超时返回 None。
#[cfg(unix)]
fn wait_listener(port: u16, timeout: std::time::Duration) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(out) = std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{port}")])
            .output()
        {
            if let Some(pid) = String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .find_map(|s| s.parse::<u32>().ok())
            {
                return Some(pid);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// 测试兜底：无条件杀掉端口上的监听进程，保证测试自身不泄漏进程。
/// 仅限测试专用端口（48765/48766），与生产清理函数的名字/ppid 校验无关。
#[cfg(unix)]
fn force_kill_listeners(port: u16) {
    if let Ok(out) = std::process::Command::new("lsof")
        .args(["-ti", &format!("tcp:{port}")])
        .output()
    {
        for pid in String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse::<u32>().ok())
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .output();
        }
    }
}

/// 准备一个名为 filemind-sidecar 的假监听进程并孤儿化（ppid→1）。
/// 通过 `sh -c 'cmd &'` 实现：sh 退出后子进程被 init/launchd 收养。
/// 返回监听 pid；未就绪（工具缺失/nc 行为差异）返回 None 并自行兜底清理。
#[cfg(unix)]
fn spawn_orphan_listener(port: u16, name: &str) -> Option<u32> {
    if !orphan_test_tools_ready() {
        return None;
    }
    let which = std::process::Command::new("which")
        .arg("nc")
        .output()
        .expect("which nc 失败");
    let nc = String::from_utf8_lossy(&which.stdout).trim().to_string();
    if nc.is_empty() {
        return None;
    }
    let tmp = tempfile::tempdir().expect("tempdir 失败");
    let fake = tmp.path().join(name);
    std::fs::copy(&nc, &fake).expect("复制 nc 失败");
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fake.metadata().expect("读 metadata 失败");
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&fake, perms).expect("设置执行位失败");
    }
    // sh 启动后台子进程后立即退出 → 子进程成孤儿（ppid=1）后监听端口。
    //
    // ⚠️ 这里**不能**用 `.output()`（或 `status()` 之外的捕获变体）：`output()` 会一直
    // 读 stdout/stderr 管道直到 EOF，而 `nc -l` 继承了这两个 fd 且长期持有不关闭，
    // 于是等待永不返回——CI Linux 上实测卡死到 job 6 小时上限（本机 macOS 的 nc
    // 行为不同才没暴露）。故把三个 stdio 全部丢弃，只等 `sh` 自身退出。
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} -l {port} &", fake.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let pid = wait_listener(port, std::time::Duration::from_secs(5));
    if pid.is_none() {
        force_kill_listeners(port);
    }
    pid
}

/// 取 pid 的 `pid/ppid/comm` 快照，供断言失败时定位（Linux 的 comm 会被截断）。
#[cfg(unix)]
fn ps_snapshot(pid: u32) -> String {
    match std::process::Command::new("ps")
        .args(["-o", "pid=,ppid=,comm=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => format!("ps 执行失败: {e}"),
    }
}

/// Linux `/proc/<pid>/comm` 限长 15 字符：`filemind-sidecar`（16 字符）实际读到的是
/// `filemind-sideca`，身份匹配必须容忍该截断，否则 Linux 上孤儿清理永远匹配不到。
#[cfg(unix)]
#[test]
fn test_matches_sidecar_comm_tolerates_linux_truncation() {
    // 完整名（macOS `ps -o comm=` 返回可执行路径，同样命中）
    assert!(matches_sidecar_comm("filemind-sidecar"));
    assert!(matches_sidecar_comm(
        "/applications/filemind.app/contents/macos/filemind-sidecar"
    ));
    // Linux 截断形态
    assert!(matches_sidecar_comm("filemind-sideca"));
    // 不相关进程名不得命中（防误杀）
    assert!(!matches_sidecar_comm("innocent-listener"));
    assert!(!matches_sidecar_comm("python3"));
}

/// BE-M3 核心场景：名为 filemind-sidecar 的孤儿（ppid=1）占住端口 → 被清理。
#[cfg(unix)]
#[test]
fn test_cleanup_orphan_sidecar_kills_named_orphan() {
    const PORT: u16 = 48765;
    let Some(pid) = spawn_orphan_listener(PORT, "filemind-sidecar") else {
        // 工具缺失或 nc 行为差异：静默跳过（本机 macOS 全量生效）
        return;
    };

    cleanup_orphan_sidecar(PORT);

    // SIGTERM 后端口应释放（轮询最多 3s）
    let gone = wait_listener(PORT, std::time::Duration::from_secs(3)).is_none();
    assert!(
        gone,
        "孤儿 Sidecar pid={pid} 应被清理，端口 {PORT} 应释放；ps: {}",
        ps_snapshot(pid)
    );
    force_kill_listeners(PORT);
}

/// 防误杀：名字不匹配的孤儿监听同端口 → 不得被清理。
#[cfg(unix)]
#[test]
fn test_cleanup_skips_unmatched_name() {
    const PORT: u16 = 48766;
    let Some(_pid) = spawn_orphan_listener(PORT, "innocent-listener") else {
        // 工具缺失或 nc 行为差异：静默跳过（本机 macOS 全量生效）
        return;
    };

    cleanup_orphan_sidecar(PORT);

    // 名字不含 filemind-sidecar → 即使是孤儿也不杀
    let still = wait_listener(PORT, std::time::Duration::from_millis(800));
    assert!(still.is_some(), "名字不匹配的进程不得被误杀");
    force_kill_listeners(PORT);
}

// ---------- BE-M7：失败的 restart 也计入 CrashLoop 窗口 ----------

/// 持续握手/spawn 失败的重启必须累计进窗口并在阈值后暂停自动恢复——
/// 旧实现只在握手成功时 `record_restart`，失败场景窗口永不增长，
/// 退避封顶 8s 后无限重试。`stub_binary` 指向无执行位的 Cargo.toml，
/// spawn 必败，正好构造「持续失败」场景。
#[tokio::test]
async fn test_restart_failure_counts_into_crash_loop_window() {
    let mut m = SidecarManager::new(stub_binary());
    for i in 0..CRASH_LOOP_MAX_RESTARTS {
        assert!(
            m.restart().await.is_err(),
            "第 {} 次 restart 应失败（占位二进制无法 spawn）",
            i + 1
        );
    }
    assert!(
        m.is_crash_loop_paused(),
        "连续 {CRASH_LOOP_MAX_RESTARTS} 次失败重启后应进入 CrashLoop 暂停，而非无限重试"
    );
}

// ---------- NO_PROXY 注入测试 ----------

/// `start()` 必须给子进程注入 `NO_PROXY` / `no_proxy`，让 Sidecar 的 httpx
/// 绕过系统代理直连本地 Ollama —— 否则 Clash 等代理会拦截 127.0.0.1 请求返回 502。
///
/// 实现思路：用一个 shell 脚本作为占位 binary，把子进程的 env 写入临时文件，
/// 然后 `start()` spawn 后读取文件校验是否包含预期的 `NO_PROXY` 值。
#[test]
fn test_start_injects_no_proxy_env() {
    // 仅 Unix：脚本通过 /bin/sh 执行
    if !cfg!(unix) {
        return;
    }

    // 1. 准备 env 输出文件（用 NamedTempFile 占位避免路径冲突，运行时换成固定路径）
    let out_dir = tempfile::tempdir().expect("建 env 输出临时目录失败");
    let out_path = out_dir.path().join("env.txt");
    let out_path_str = out_path.to_str().unwrap().to_string();

    // 2. 写占位 sidecar 脚本：把 env 输出到 FILEMIND_TEST_ENV_OUT 指向的文件
    let script = format!(
        "#!/bin/sh\nenv > \"{}\"\n",
        out_path_str.replace('"', "\\\"")
    );
    let script_dir = tempfile::tempdir().expect("建脚本临时目录失败");
    let script_path = script_dir.path().join("fake-sidecar.sh");
    std::fs::write(&script_path, &script).expect("写脚本失败");
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&script_path).expect("读脚本 metadata 失败");
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(&script_path, perms).expect("脚本设可执行位失败");
    }

    // 3. 让子进程继承 FILEMIND_TEST_ENV_OUT（脚本依赖此变量定位输出文件）
    let _guard = EnvGuard::set("FILEMIND_TEST_ENV_OUT", &out_path_str);

    // 4. start() spawn 脚本 → 脚本执行 env 写文件后退出
    let mut m = SidecarManager::new(script_path);
    let psk = m.start().expect("spawn 占位脚本应成功");

    // 5. wait 子进程退出（脚本极快，几乎立即完成），确保 env 文件已写完
    if let Some(child) = m.process.as_mut() {
        let _ = child.wait();
    }

    // 6. 校验：子进程 env 中应包含 NO_PROXY 与 no_proxy（大小写双写）
    let env_content = std::fs::read_to_string(&out_path).unwrap_or_else(|_| String::new());
    assert!(
        env_content.contains("NO_PROXY=127.0.0.1,localhost,::1"),
        "子进程应继承 NO_PROXY=127.0.0.1,localhost,::1，实际 env:\n{env_content}"
    );
    assert!(
        env_content.contains("no_proxy=127.0.0.1,localhost,::1"),
        "子进程应继承 no_proxy=127.0.0.1,localhost,::1（小写），实际 env:\n{env_content}"
    );
    // PSK 仍是 32 字节随机，注入 env 不影响握手协议
    assert_eq!(
        psk.len(),
        32,
        "PSK 应为 32 字节，注入 env 不应破坏 PSK 生成"
    );
}
