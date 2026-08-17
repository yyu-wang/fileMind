# 错误处理闭环体系

> 企业级开发要求：错误从产生到用户感知的完整链路，不吞错、不静默失败、有错误码、有用户友好提示。

## 错误码体系

### 错误码格式

```
{模块}-{错误类型}-{序号}

模块:
  FILE  - 文件操作
  SCAN  - 文件扫描
  CLS   - 分类引擎
  RAG   - RAG 问答
  EMB   - Embedding
  SEC   - 安全
  DB    - 数据库
  NET   - 网络
  SIDE  - Sidecar
  CFG   - 配置

错误类型:
  E - 预期错误（用户可纠正）
  U - 非预期错误（系统故障）
  P - 权限错误
  V - 校验错误
```

### 错误码注册表

| 错误码 | 模块 | 消息（用户可见） | 内部描述 | HTTP/IPC |
|--------|------|-------------------|----------|----------|
| FILE-E-001 | 文件操作 | "文件不存在" | 路径校验通过但文件已删除 | 404 |
| FILE-E-002 | 文件操作 | "文件已被其他程序占用" | 文件锁冲突 | 409 |
| FILE-P-001 | 文件操作 | "没有权限访问该目录" | 路径在黑名单中 | 403 |
| FILE-V-001 | 文件操作 | "文件大小超过限制（最大 100MB）" | 文件超限 | 413 |
| SCAN-E-001 | 扫描 | "目录为空" | 扫描结果为空 | 200 |
| SCAN-U-001 | 扫描 | "扫描中断，请重试" | IO 错误中断 | 500 |
| CLS-E-001 | 分类 | "无法识别文件类型" | 扩展名不在支持列表 | 422 |
| CLS-U-001 | 分类 | "分类服务暂时不可用" | LLM/Ollama 不可用 | 503 |
| RAG-U-001 | RAG | "问答服务暂时不可用" | Embedding/LLM 错误 | 503 |
| EMB-U-001 | Embedding | "索引服务暂时不可用" | LanceDB/Ollama 错误 | 503 |
| SEC-P-001 | 安全 | "操作需要确认" | 模式切换需同意 | 403 |
| DB-U-001 | 数据库 | "数据读取失败，请重启应用" | SQLite 错误 | 500 |
| NET-U-001 | 网络 | "网络连接失败" | reqwest 错误 | 502 |
| SIDE-U-001 | Sidecar | "AI 服务启动失败" | Sidecar 进程崩溃 | 503 |
| CFG-V-001 | 配置 | "配置项无效" | Pydantic 校验失败 | 422 |

## Rust 错误处理闭环

### 错误产生 → 传播 → 用户感知

```rust
// 1. 错误产生（底层模块）
pub fn scan_files(root: &Path) -> AppResult<Vec<FileInfo>> {
    let entries = std::fs::read_dir(root)
        .map_err(|e| AppError::Io(e))?;  // 不吞错，链式传播
    // ...
}

// 2. IPC 层转换（给前端的错误）
#[tauri::command]
pub async fn scan_directory(path: String) -> Result<ScanResponse, String> {
    let safe_path = path_guard::validate(&path)
        .map_err(|e| {
            // 安全错误 → 用户友好消息
            error!("Path validation failed: {}", e);
            match e {
                AppError::UnsafePath(_) => "FILE-P-001:没有权限访问该目录".to_string(),
                _ => "SCAN-U-001:扫描中断，请重试".to_string(),
            }
        })?;

    let files = scan_files(&safe_path)
        .map_err(|e| {
            error!("Scan failed: {}", e);
            "SCAN-U-001:扫描中断，请重试".to_string()
        })?;

    Ok(ScanResponse { files, error_code: None })
}

// 3. 前端处理（用户感知）
async function handleScan(path: string) {
  try {
    const result = await ipc.scanDirectory(path);
    // 正常流程
  } catch (error) {
    const [code, message] = parseErrorCode(error);
    // code = "FILE-P-001", message = "没有权限访问该目录"
    toast.error(message);  // 用户看到友好消息
    logger.error('Scan failed', { code, path });  // 日志记录错误码
  }
}
```

### 错误处理规则

| 规则 | 说明 |
|------|------|
| 不吞错 | 每个 `?` 必须有对应的错误转换逻辑，不直接 `.unwrap_or_default()` |
| 错误分级 | 预期错误（E）返回用户友好消息；非预期错误（U）记录详细日志+通用提示 |
| 错误码 | 所有返回给前端的错误必须带错误码 |
| 日志脱敏 | error!() 日志中不包含文件路径、API Key 明文 |
| 重试策略 | 网络/Sidecar 错误自动重试 3 次，指数退避 |

## Python 错误处理闭环

### FastAPI 异常处理

```python
from fastapi import HTTPException, Request
from fastapi.responses import JSONResponse
import logging

logger = logging.getLogger(__name__)

# 自定义异常
class FileMindError(Exception):
    """FileMind 业务异常基类。"""
    def __init__(self, code: str, message: str, status_code: int = 500):
        self.code = code
        self.message = message
        self.status_code = status_code
        super().__init__(message)

class ClassificationError(FileMindError):
    """分类引擎异常。"""
    pass

# 全局异常处理器
@app.exception_handler(FileMindError)
async def filemind_error_handler(request: Request, exc: FileMindError) -> JSONResponse:
    """处理业务异常，返回错误码和用户友好消息。"""
    logger.error(
        "Business error: code=%s path=%s",
        exc.code,
        sanitize_path(request.url.path),
    )
    return JSONResponse(
        status_code=exc.status_code,
        content={
            "error_code": exc.code,
            "message": exc.message,
        },
    )

# 路由中使用
@router.post("", response_model=ClassifyResponse)
async def classify_files(request: ClassifyRequest) -> ClassifyResponse:
    if not request.files:
        raise FileMindError("CLS-E-001", "无法识别文件类型", 422)

    try:
        result = await classifier.classify(request.files)
    except OllamaUnavailableError as e:
        logger.error("Ollama unavailable: %s", e)
        raise FileMindError("CLS-U-001", "分类服务暂时不可用", 503) from e

    return ClassifyResponse(items=result.items)
```

## 前端错误处理闭环

### 错误边界 + Toast + 日志

```tsx
// 1. 错误边界（捕获渲染错误）
interface ErrorBoundaryState {
  hasError: boolean;
  errorCode: string | null;
}

export class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { hasError: false, errorCode: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, errorCode: 'RENDER-U-001' };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    logger.error('Render error', { error: error.message, info });
  }

  render() {
    if (this.state.hasError) {
      return <ErrorFallback code={this.state.errorCode} />;
    }
    return this.props.children;
  }
}

// 2. IPC 错误处理 Hook
export function useIpcErrorHandler() {
  return useCallback((error: unknown, context: string) => {
    const message = parseErrorMessage(error);
    const code = parseErrorCode(error);
    toast.error(message);
    logger.error(context, { code, message });
  }, []);
}

// 3. 错误码解析
function parseErrorCode(error: unknown): string {
  if (typeof error === 'string') {
    const match = error.match(/^([A-Z]+-[A-Z]-\d+)/);
    return match?.[1] ?? 'UNKNOWN';
  }
  return 'UNKNOWN';
}
```

### 前端错误展示规则

| 错误类型 | 展示方式 | 用户操作 |
|----------|----------|----------|
| 权限错误 (P) | Modal 弹窗 | 引导修改权限 |
| 校验错误 (V) | Inline 表单错误 | 修正输入 |
| 预期错误 (E) | Toast (3s) | 无需操作 |
| 非预期错误 (U) | Toast + 重试按钮 | 点击重试 |
| 渲染错误 | 全屏 ErrorFallback | 点击刷新 |
