//! Rust 侧日志落盘：把应用日志同时写到 stderr 与 `{数据目录}/logs/filemind.log`。
//!
//! 背景（2026-09-16 实机排查）：`env_logger` 原先只写 stderr，而打包版由 GUI 双击
//! 启动、stderr 无处可看，于是 watchdog 的「需要重启 / 重启失败 / CrashLoop」这些
//! 关键事件事后完全无迹可查——只能靠猜 sidecar 为什么死。
//!
//! 约定：
//! - 路径 `{数据目录}/logs/filemind.log`，与 sidecar 日志同目录（`~/.filemind/logs`）；
//! - 单文件超过 [`MAX_LOG_BYTES`] 时轮转为 `.1`（只留一份历史，避免无界增长）；
//! - 只写文件不写 stderr 会丢终端可读性，故用 [`TeeWriter`] 双写；
//! - 数据目录不可解析 / 文件不可写时降级为「只写 stderr」，不阻断启动。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 单文件上限（字节）：启动时超过该值先把现行日志轮转为 `.1`。
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// 轮转备份扩展名：`filemind.log` → `filemind.log.1`。
const ROTATED_EXT: &str = "log.1";

/// 数据目录（与 `main.rs::get_db_path`、`sidecar/platform.rs` 同一约定）：
/// `FILEMIND_DATA_HOME` 优先，未设置时回退 `~/.filemind`。
#[must_use]
pub fn data_home() -> Option<PathBuf> {
    std::env::var("FILEMIND_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".filemind")))
}

/// 初始化 Rust 侧日志：脱敏 formatter + 默认 `warn` 级 + 同时写 stderr 与文件。
///
/// 在 `main()` 最早期调用（早于一切 `log::` 输出）。要点：
/// - formatter 走 `log_redact` 单一出口脱敏（安全 I-03），文件里落的是同一份已脱敏文本；
/// - 默认 `warn`：watchdog 的「需要重启 / 重启失败 / CrashLoop」全在此级别以上，
///   避免默认 `error` 级把关键事件漏掉；`RUST_LOG` 存在时由 `parse_default_env` 覆盖；
/// - 文件不可写时降级为只写 stderr，并（logger 就绪后）打一条告警。
pub fn init_logger() {
    let (writer, warning) = build_writer();
    env_logger::Builder::new()
        .format(|buf, record| {
            let message = crate::security::log_redact::redact(&record.args().to_string());
            writeln!(buf, "[{} {}] {}", record.level(), record.target(), message)
        })
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .target(env_logger::Target::Pipe(writer))
        .init();

    if let Some(warning) = warning {
        log::warn!("{warning}");
    }
}

/// 应用日志文件路径：`{数据目录}/logs/filemind.log`。
#[must_use]
pub fn log_path(data_home: &Path) -> PathBuf {
    data_home.join("logs").join("filemind.log")
}

/// 构造 `env_logger` 输出目标用的 writer，并返回启动期告警（供 logger 就绪后打印）。
///
/// 返回 `(writer, warning)`：`warning` 非空表示文件输出不可用（目录不可写 / 打开失败），
/// 此时仍返回只写 stderr 的 writer —— 日志功能降级，但应用照常启动。
#[must_use]
pub fn build_writer() -> (Box<dyn Write + Send>, Option<String>) {
    let Some(home) = data_home() else {
        return (
            Box::new(TeeWriter::new(None)),
            Some("日志目录解析失败（FILEMIND_DATA_HOME 与家目录均不可用），日志仅写 stderr".into()),
        );
    };
    match open_file(&home) {
        Ok(file) => (Box::new(TeeWriter::new(Some(file))), None),
        Err(e) => (
            Box::new(TeeWriter::new(None)),
            Some(format!("日志文件不可写，仅写 stderr: {e}")),
        ),
    }
}

/// 打开（必要时轮转）日志文件：目录缺失自动创建，超限先轮转为 `.1`。
///
/// # Errors
///
/// 目录创建或文件打开失败时返回底层 IO 错误。
fn open_file(data_home: &Path) -> io::Result<File> {
    let path = log_path(data_home);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    rotate_if_needed(&path);
    OpenOptions::new().create(true).append(true).open(&path)
}

/// 超过 [`MAX_LOG_BYTES`] 时把现行日志改名为 `.1`（覆盖旧的备份）。
///
/// 轮转失败（文件被占用等）不报错：继续追加写原文件即可，只是体积不受控。
fn rotate_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return; // 文件不存在 → 无需轮转
    };
    if meta.len() < MAX_LOG_BYTES {
        return;
    }
    let rotated = path.with_extension(ROTATED_EXT);
    let _ = fs::rename(path, rotated);
}

/// 同时写 stderr 与文件的 writer。
///
/// stderr 先写：即使落盘失败，终端 / 父进程仍能看到日志；文件写失败一次即停用
/// 文件输出，避免每行都重复报错刷屏。
pub struct TeeWriter {
    file: Option<File>,
}

impl TeeWriter {
    /// 用给定文件构造；`None` 表示只写 stderr。
    #[must_use]
    pub const fn new(file: Option<File>) -> Self {
        Self { file }
    }

    /// 当前是否已接上文件输出（供单测断言，不影响生产逻辑）。
    #[must_use]
    pub const fn has_file(&self) -> bool {
        self.file.is_some()
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = io::stderr().write(buf)?;
        if let Some(file) = self.file.as_mut() {
            if let Err(e) = file.write_all(&buf[..written]) {
                // 此处不能用 log（本类型就是日志汇聚点，会递归），故直接写 stderr
                let _ = writeln!(io::stderr(), "日志落盘失败，后续仅写 stderr: {e}");
                self.file = None;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()?;
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// 打开日志文件时自动创建 `logs` 目录。
    #[test]
    fn open_creates_log_dir() {
        let dir = std::env::temp_dir().join(format!("filemind-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = open_file(&dir).unwrap();
        drop(file);
        assert!(log_path(&dir).is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 超过上限 → 轮转为 `.1`，且新内容写回原文件。
    #[test]
    fn oversized_file_is_rotated() {
        let dir = std::env::temp_dir().join(format!("filemind-log-rot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = log_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();

        let file = open_file(&dir).unwrap();
        drop(file);

        assert!(
            path.with_extension(ROTATED_EXT).is_file(),
            "旧日志应轮转为 .1"
        );
        assert!(log_path(&dir).is_file(), "轮转后应重建当前日志文件");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 未超上限不轮转。
    #[test]
    fn small_file_is_not_rotated() {
        let dir = std::env::temp_dir().join(format!("filemind-log-small-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = log_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"hello").unwrap();

        rotate_if_needed(&path);

        assert!(!path.with_extension(ROTATED_EXT).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 双写：内容同时进入文件；无文件时也不 panic（只写 stderr）。
    #[test]
    fn tee_writer_writes_to_file() {
        let dir = std::env::temp_dir().join(format!("filemind-log-tee-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = open_file(&dir).unwrap();

        let mut writer = TeeWriter::new(Some(file));
        assert!(writer.has_file());
        writer.write_all(b"line-1\n").unwrap();
        writer.flush().unwrap();
        drop(writer);

        let content = fs::read_to_string(log_path(&dir)).unwrap();
        assert_eq!(content, "line-1\n");
        let _ = fs::remove_dir_all(&dir);

        let mut no_file = TeeWriter::new(None);
        assert!(!no_file.has_file());
        assert_eq!(no_file.write(b"only-stderr\n").unwrap(), 12);
    }
}
