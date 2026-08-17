---
name: "create-error-code"
description: "Creates a new error code in the error registry with Rust error variant, Python exception, and frontend handling. Invoke when adding new error scenarios to the application."
---

# Create Error Code

Creates a new error code following `rules/error-handling.md` error code system.

## When to Invoke

- Adding a new error scenario
- User asks to create an error code
- Adding a new IPC command that can fail in new ways
- Adding a new Python API endpoint with new error cases

## Steps

### 1. Read Rules First

Read `rules/error-handling.md` to understand:
- Error code format: `{MODULE}-{TYPE}-{NUM}`
- Module codes: FILE, SCAN, CLS, RAG, EMB, SEC, DB, NET, SIDE, CFG
- Type codes: E (expected), U (unexpected), P (permission), V (validation)
- Error handling chain: Rust → IPC → Frontend

### 2. Determine Error Code

Check existing error codes in `rules/error-handling.md` error registry.
Determine:
- Module: which domain does this error belong to?
- Type: is it expected (E), unexpected (U), permission (P), or validation (V)?
- Number: next sequential number for this module+type combo

### 3. Update Error Registry

Add to the error code table in `rules/error-handling.md`:

```markdown
| {MODULE}-{TYPE}-{NUM} | {模块} | "{用户可见消息}" | {内部描述} | {HTTP状态码} |
```

### 4. Add Rust Error Variant

Add to `src-tauri/src/error.rs`:

```rust
#[derive(Debug, Error)]
pub enum AppError {
    // ... existing variants

    #[error("{code}: {message}")]
    BusinessError {
        code: String,
        message: String,
    },
}
```

### 5. Add IPC Error Mapping

In the IPC command, map the error to the error code:

```rust
#[tauri::command]
#[specta::specta]
pub async fn {command}({params}) -> Result<{ReturnType}, String> {
    let result = do_something()
        .map_err(|e| {
            error!("Operation failed: {}", e);
            format!("{MODULE}-{TYPE}-{NUM}:{user_message}")
        })?;
    Ok(result)
}
```

### 6. Add Python Exception (if Sidecar endpoint)

If the error originates from Python:

```python
class {ModuleName}Error(FileMindError):
    """{Module} 业务异常。"""
    pass

# In route handler
@router.post("")
async def endpoint(request: RequestType) -> ResponseType:
    try:
        result = await service.process(request)
    except SpecificError as e:
        logger.error("Operation failed: %s", e)
        raise {ModuleName}Error(
            code="{MODULE}-{TYPE}-{NUM}",
            message="{用户可见消息}",
            status_code={HTTP_CODE},
        ) from e
```

### 7. Add Frontend Error Handling

Add to the frontend error handling utility:

```tsx
// lib/utils/errorCodes.ts
export const ERROR_CODES = {
  '{MODULE}-{TYPE}-{NUM}': {
    message: '{用户可见消息}',
    display: 'toast' | 'modal' | 'inline',  // 根据类型选择
    retryable: true | false,
  },
} as const;

export function handleIpcError(error: unknown, context: string): void {
  const code = parseErrorCode(error);
  const config = ERROR_CODES[code];

  if (config) {
    if (config.display === 'modal') {
      showModal(config.message);
    } else if (config.display === 'inline') {
      // Set inline error state
    } else {
      toast.error(config.message);
    }

    if (config.retryable) {
      toast.action('重试', () => retryOperation(context));
    }
  } else {
    toast.error('未知错误，请稍后重试');
  }

  logger.error(context, { code, rawError: error });
}
```

### 8. Error Code Quality Checklist

- [ ] Error code follows format `{MODULE}-{TYPE}-{NUM}`
- [ ] Module code is from the standard list (FILE/SCAN/CLS/RAG/EMB/SEC/DB/NET/SIDE/CFG)
- [ ] Type code is correct (E/U/P/V)
- [ ] Number is sequential (no gaps)
- [ ] User-facing message is friendly (no technical jargon, no English)
- [ ] Internal description is detailed (for debugging)
- [ ] HTTP status code is correct (403/404/422/500/502/503)
- [ ] Rust error variant added
- [ ] Python exception added (if Sidecar endpoint)
- [ ] Frontend error handling added
- [ ] Error code registered in `rules/error-handling.md` table
- [ ] Log doesn't expose sensitive data (paths, keys, content)

### 9. Add Test

```rust
#[tokio::test]
async fn test_{command}_returns_error_code() {
    let result = {command}(invalid_input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("{MODULE}-{TYPE}-{NUM}"));
}
```

### 10. Self-Check

```bash
# Rust
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml

# TypeScript
npx eslint src/lib/utils/errorCodes.ts --max-warnings 0
npx tsc --noEmit
```
