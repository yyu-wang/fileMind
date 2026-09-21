---
name: 'create-ipc-command'
description: 'Creates a new Tauri IPC command with specta types, path validation, error handling, and test scaffold. Invoke when adding a new Rust IPC command for frontend-backend communication.'
---

# Create IPC Command

Creates a new Tauri IPC command following project security and type-safety standards.

## When to Invoke

- User asks to create a new IPC command
- User asks to add a new Tauri command
- Adding frontend-backend communication endpoint

## Steps

### 1. Read Rules First

Before generating any code, read these rule files:

- `rules/rust.md` — Rust coding rules (no unwrap, Result<T,String>, specta required)
- `rules/security.md` — Security rules (path validation, key management)

### 2. Determine Command Details

Ask or infer:

- Command name (snake_case, e.g. `scan_directory`)
- Parameters (with types)
- Return type
- Whether it involves file paths (requires `path_guard::validate()`)
- Whether it's async (IO/network operations)

### 3. Generate Command Code

Write to `src-tauri/src/commands/{module}.rs`:

```rust
use crate::error::AppResult;
use crate::security;
use serde::{Deserialize, Serialize};

#[tauri::command]
#[specta::specta]
pub async fn {command_name}({params}) -> Result<{return_type}, String> {
    // If path parameter exists — MUST validate
    {path_validation_line}

    // Business logic here
    {business_logic}

    Ok(result)
}
```

### 4. Security Checklist (MUST verify)

- [ ] All path parameters pass through `security::validate()` or `security::validate_within_root()`
- [ ] Return type is `Result<T, String>` (not `T` directly)
- [ ] No `unwrap()` / `expect()` / `panic!()` — use `?` operator
- [ ] `#[specta::specta]` derive macro is present
- [ ] All types derive `Serialize`, `Deserialize`, and `specta::Type`
- [ ] Async operations use `async fn`

### 5. Register Command

Add to the command table in `src-tauri/src/ipc_handler.rs`（**不是** `src-tauri/src/main.rs`——清单已独立成 bin 模块，main.rs 只调用 `ipc_handler::build()`）:

```rust
tauri::generate_handler![
    // ... existing commands
    commands::{module}::{command_name},
]
```

Add to `src-tauri/examples/export_specta.rs`（`collect_commands!`；与上面清单保持同一顺序，便于对照维护）:

```rust
commands::{module}::{command_name},
```

> ⚠️ 路径必须是**命令定义所在的子模块全路径**（如 `commands::file_ops::scan::scan_directory`），
> 不能用再导出后的短路径（`commands::file_ops::scan_directory`）。
> `#[tauri::command]` / `#[specta::specta]` 生成的隐藏辅助项（`__tauri_command_name_*` /
> `__cmd__*` / `__specta__fn__*`）只存在于命令定义所在模块，宏按「给定路径的父模块」
> 查找它们，`pub use` 再导出带不过去，编译会报 `cannot find __cmd__xxx`。

### 6. Generate Test Scaffold

Create test in `src-tauri/src/commands/{module}_tests.rs` or inline `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_{command_name}_success() {
        // Arrange
        // Act
        // Assert
    }

    #[tokio::test]
    async fn test_{command_name}_invalid_path() {
        // Path validation should reject unsafe paths
    }
}
```

### 7. Regenerate IPC Types

```bash
make gen-ipc
```

### 8. Self-Check

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Both MUST pass with zero warnings.
