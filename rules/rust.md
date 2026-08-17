# Rust / Tauri 编码规则

> 来源：06_工程化基础规范.html §4 + AGENTS.md

## 绝对禁止

| 规则 | 说明 | 检查方式 |
|------|------|----------|
| 禁止 `unwrap()` | 生产代码中完全禁止 | Clippy `unwrap_used = "deny"` |
| 禁止 `expect()` | 生产代码中完全禁止 | Clippy `expect_used = "deny"` |
| 禁止 `panic!()` | 生产代码中完全禁止 | Clippy `panic = "deny"` |
| 禁止 `dbg!()` | 使用 `log` crate | Clippy `dbg_macro = "deny"` |
| 禁止 `println!()` | 使用 `log` crate | Clippy `print_stdout = "deny"` |
| 禁止 `unsafe` | 除非有安全审查 | Rust lint `unsafe_code = "deny"` |
| 禁止 `#[allow(...)]` 绕过 clippy | 修复问题而非抑制警告 | Code Review |

## 强制要求

### 错误处理
```rust
// error.rs — 统一错误类型
#[derive(Debug, Error)]
pub enum AppError {
    #[error("路径不安全: {0}")]
    UnsafePath(String),
    #[error("Sidecar 不可用: {0}")]
    SidecarUnavailable(String),
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("推理模式切换被拒绝: 当前 {current} 模式不可自动切换")]
    ModeSwitchForbidden { current: String },
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

### IPC 命令规范
```rust
// ✅ 正确：specta 派生 + 路径校验 + Result 返回
#[tauri::command]
#[specta::specta]
pub async fn scan_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let safe_path = path_guard::validate(&path)?;  // 不用 unwrap
    let files = db::scan_files(&safe_path).await?;
    Ok(files)
}
```

| 规则 | 说明 |
|------|------|
| 命名 | snake_case（`scan_directory`, `preview_operations`） |
| 类型标注 | 必须加 `#[specta::specta]` 派生宏 |
| 返回类型 | 统一 `Result<T, String>` |
| 参数校验 | 所有路径参数必须经 `path_guard::validate()` |
| 异步 | 涉及 IO/网络用 `async fn` |
| 权限 | 在 `capabilities/` 中声明 |

### 模块组织
- 每个功能模块一个目录，含 `mod.rs` 导出公共接口
- `pub use` 重导出公共类型，隐藏内部实现
- 跨模块引用走 `crate::module::Type` 完整路径

### Clippy 配置
```toml
# Cargo.toml [lints.clippy]
all = "deny"
pedantic = "warn"
nursery = "warn"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "warn"
```

### 复杂度限制
```toml
# clippy.toml
cognitive-complexity-threshold = 15
too-many-arguments-threshold = 6
type-complexity-threshold = 250
enum-variant-size-threshold = 200
```
