# 安全红线规则

> 来源：07_安全合规设计.html + AGENTS.md

## 路径安全

### 规则
所有用户输入的文件路径必须经过 `path_guard::validate()` 校验后才能使用。

### 正确示例
```rust
// ✅ 正确：路径经过校验
#[tauri::command]
pub async fn scan_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let safe_path = path_guard::validate(&path)?;
    let files = scan_files(&safe_path)?;
    Ok(files)
}

// ✅ 正确：子路径校验根目录
let safe_path = path_guard::validate_within_root(&sub_path, &root)?;
```

### 错误示例
```rust
// ❌ 错误：直接使用用户输入的路径
let files = std::fs::read_dir(&path)?;

// ❌ 错误：跳过校验拼接路径
let full_path = format!("{base}/{user_input}");
```

### 黑名单路径
以下路径被安全策略阻止，不可扫描/操作：
- `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/dev`, `/proc`, `/sys` (macOS/Linux)
- `C:\Windows\System32`, `C:\Program Files` (Windows)

## 密钥安全

### 规则
API Key 存储在 OS Keychain 中，通过 Rust 代理转发给云端 API，不经过 Python Sidecar。

### 正确示例
```rust
// ✅ 正确：密钥从 Keychain 读取，在 Rust 端发起请求
let key = keychain::get_key("openai")?;
let response = reqwest::post(url)
    .header("Authorization", format!("Bearer {key}"))
    .send()
    .await?;
```

### 错误示例
```rust
// ❌ 错误：密钥硬编码
const API_KEY = "sk-...";

// ❌ 错误：密钥传给 Python
sidecar_proxy::forward_post("/classify", &format!(r#"{{"api_key": "{key}"}}"#));
```

### Keychain 操作
```rust
// 存储
keychain::store_key("openai", "sk-xxx")?;

// 读取
let key = keychain::get_key("openai")?; // Option<String>

// 删除
keychain::delete_key("openai")?;
```

## 推理模式切换安全

### 规则
从 local 切换到 cloud 模式需要用户知情同意，自动切换需校验同意状态。

### 正确示例
```rust
// ✅ 正确：模式切换经过校验
security::mode_switch::validate_mode_switch("local", "cloud", &source)?;
// 如果 source == "auto" 且无同意 → 返回 ModeSwitchForbidden
```

### 安全阀
```rust
// MODE_SWITCH_FORBIDDEN 安全阀
// 如果当前模式不可自动切换，返回错误
if source == "auto" && !has_consent() {
    return Err(AppError::ModeSwitchForbidden { current: current.to_string() });
}
```

## 云端数据脱敏

### 规则
云端模式下发送的数据必须脱敏：文件名编号化、路径脱敏、内容截断。

```python
# ✅ 正确：云端请求脱敏
def sanitize_for_cloud(file_info: FileInfo) -> dict:
    return {
        "file_id": f"file_{hashlib.md5(file_info.path.encode()).hexdigest()[:8]}",
        "file_name": f"file_{file_info.id[:6]}",  # 编号化
        "content": file_info.content[:2000],       # 截断 2000 字符
        "path": "***",                              # 路径脱敏
    }
```

## 日志脱敏

### 规则
日志中不得出现 API Key、文件路径、查询内容的明文。

```python
# ✅ 正确：日志脱敏
logger.info("Processing file: %s", sanitize_path(file_path))
logger.debug("API response received from %s", provider)

# ❌ 错误：日志泄露敏感信息
logger.info("Processing file: %s", file_path)  # 真实路径
logger.debug("API key: %s", api_key)           # API Key 明文
```

## 日志链式哈希

### 规则
文件操作日志使用 SHA-256 链式哈希，每次启动时校验完整性。

```sql
-- operations_log 表中的 prev_hash 和 current_hash
-- current_hash = SHA256(prev_hash + operation_data)
-- 启动时从第一条开始校验链式完整性
```
