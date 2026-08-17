# 可观测性规范

> 企业级开发要求：日志分级、指标埋点、链路追踪，问题可定位。

## 日志体系

### 日志级别

| 级别 | 使用场景 | 示例 |
|------|----------|------|
| `error` | 系统故障、不可恢复错误 | Sidecar 崩溃、DB 写入失败 |
| `warn` | 可恢复的异常、降级行为 | LLM 超时降级到规则匹配 |
| `info` | 关键业务节点 | 扫描完成、分类完成、模式切换 |
| `debug` | 调试信息（生产关闭） | 中间结果、参数值 |
| `trace` | 详细执行流程（仅开发） | 函数进出、SQL 语句 |

### 日志格式

```rust
// Rust — 使用 log crate + env_logger
use log::{info, warn, error};

info!("Scan completed: path={}, files={}, duration_ms={}",
    sanitize_path(&path),    // 脱敏
    file_count,
    elapsed.as_millis()
);

warn!("LLM timeout, falling back to rules: file_id={}, retry={}",
    file_id,  // ID 不脱敏
    retry_count
);

error!("Sidecar crashed: exit_code={}, stderr={}",
    exit_code,
    stderr_lines  // 不含用户数据
);
```

```python
# Python — 使用 logging
import logging

logger = logging.getLogger(__name__)

logger.info(
    "Classification completed: file_count=%d, rule_hit=%d, llm_hit=%d, duration_ms=%d",
    file_count, rule_hit, llm_hit, duration_ms,
)

logger.warning(
    "Ollama timeout, falling back to rules: model=%s, timeout=%d",
    model_name,  # 模型名不脱敏
    timeout_sec,
)
```

### 日志脱敏规则

| 数据类型 | 脱敏方式 | 示例 |
|----------|----------|------|
| 文件路径 | `sanitize_path()` 只保留最后一级 | `/Users/x/docs` → `***/docs` |
| 文件名 | 编号化 | `report.pdf` → `file_abc123` |
| API Key | 完全不记录 | — |
| 查询内容 | 完全不记录 | — |
| 文件内容 | 完全不记录 | — |
| 错误码 | 原样记录 | `CLS-U-001` |
| 耗时/计数 | 原样记录 | `duration_ms=350` |

### 日志输出位置

| 环境 | Rust 日志 | Python 日志 | 级别 |
|------|-----------|-------------|------|
| 开发 | stdout | stdout | debug |
| 生产 | `~/.filemind/logs/app.log` | `~/.filemind/logs/sidecar.log` | info |
| CI | stdout | 不适用 | warn |

日志轮转：单文件最大 10MB，保留最近 5 个文件。

## 指标埋点

### 关键业务指标

```rust
// Rust — 埋点结构
#[derive(Serialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub tags: HashMap<String, String>,
}

// 扫描指标
metrics::record(Metric {
    name: "scan.duration_ms".to_string(),
    value: elapsed.as_millis() as f64,
    unit: "ms".to_string(),
    tags: hashmap! { "file_count" => count.to_string() },
});

// 分类指标
metrics::record(Metric {
    name: "classify.rule_hit_rate".to_string(),
    value: rule_hit as f64 / total as f64,
    unit: "ratio".to_string(),
    tags: hashmap! {},
});
```

### 指标清单

| 指标名 | 类型 | 说明 | 采集点 |
|--------|------|------|--------|
| `scan.duration_ms` | 计时 | 扫描耗时 | scan_directory |
| `scan.file_count` | 计数 | 扫描文件数 | scan_directory |
| `classify.duration_ms` | 计时 | 分类耗时 | classify_files |
| `classify.rule_hit_rate` | 比率 | 规则层命中率 | classify_files |
| `classify.llm_fallback_rate` | 比率 | LLM 兜底比例 | classify_files |
| `rag.first_token_ms` | 计时 | 首 token 延迟 | chat_query |
| `rag.total_duration_ms` | 计时 | 问答总耗时 | chat_query |
| `sidecar.startup_ms` | 计时 | Sidecar 启动耗时 | SidecarManager |
| `sidecar.crash_count` | 计数 | 崩溃次数 | SidecarManager |
| `db.query_ms` | 计时 | DB 查询耗时 | db module |

## 链路追踪

### 请求 ID 串联

```
前端 IPC 调用 → Rust 生成 request_id → 传递给 Python Sidecar → 日志串联
```

```rust
// Rust — 生成 request_id 并传递
let request_id = uuid::Uuid::new_v4().to_string();
info!("IPC request started: id={}, command={}", request_id, "scan_directory");

// 传递给 Sidecar
let response = sidecar::proxy::forward_post_with_header(
    "/classify",
    &body,
    "X-Request-ID",
    &request_id,
).await?;
```

```python
# Python — 接收 request_id 并记录
from fastapi import Request

@router.post("/classify")
async def classify(request: Request, body: ClassifyRequest) -> ClassifyResponse:
    request_id = request.headers.get("X-Request-ID", "unknown")
    logger.info("Classify request: id=%s, file_count=%d", request_id, len(body.files))
    # ...
```

### 追踪日志格式

```
2026-08-17T10:30:00Z INFO  [req_id=abc123] IPC scan_directory started: path=***/docs
2026-08-17T10:30:01Z INFO  [req_id=abc123] Scan completed: files=1523, duration_ms=850
2026-08-17T10:30:02Z INFO  [req_id=abc123] Classify request sent to sidecar
2026-08-17T10:30:03Z INFO  [req_id=abc123] Classify completed: rule_hit=1200, llm_hit=323, duration_ms=1200
```

## 健康检查

### Sidecar 健康检查

```python
# python-sidecar/app/api/routes_health.py
@router.get("/health")
async def health_check() -> HealthResponse:
    """Sidecar 健康检查，含组件状态。"""
    return HealthResponse(
        status="ok",
        version="0.1.0",
        uptime_seconds=uptime,
        components={
            "ollama": check_ollama_health(),     # 检查 Ollama 连接
            "lancedb": check_lancedb_health(),   # 检查 LanceDB 连接
            "models": check_models_loaded(),     # 检查模型加载状态
        },
    )
```

### Rust 健康检查

```rust
// 每 30 秒检查 Sidecar 健康
pub async fn health_check_loop(sidecar: &SidecarManager) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if !sidecar.health_check().await.unwrap_or(false) {
            error!("Sidecar health check failed, attempting restart");
            if let Err(e) = sidecar.restart().await {
                error!("Sidecar restart failed: {}", e);
                // 发射事件通知前端
                app.emit("sidecar:status", SidecarStatusEvent {
                    status: "crashed".to_string(),
                    message: Some("AI 服务崩溃，正在尝试恢复".to_string()),
                }).ok();
            }
        }
    }
}
```
