# Python / FastAPI 编码规则

> 来源：06_工程化基础规范.html §5 + AGENTS.md

## 绝对禁止

| 规则 | 说明 | 检查方式 |
|------|------|----------|
| 禁止 `Any` 类型 | 用 `object` 或泛型替代 | mypy `disallow_any_explicit = true` |
| 禁止 `# type: ignore` | 修复类型问题而非抑制 | ruff + mypy |
| 禁止 `print()` | 使用 `logging` | Code Review |
| 禁止手动解析 JSON | 使用 Pydantic 模型 | Code Review |
| 禁止在 Sidecar 存储 API Key | 密钥通过 Rust Keychain 管理 | 架构约束 |
| 禁止同步 IO | 所有 IO 操作用 `async def` | Code Review |

## 强制要求

### 类型标注
```python
# ✅ 所有函数参数和返回值必须有类型标注（mypy strict）
async def classify_files(
    files: list[FileInfo],
    strategy: ClassifyStrategy,
) -> ClassifyResponse:
    """对文件列表执行三层分类。

    Args:
        files: 待分类的文件列表
        strategy: 分类策略配置

    Returns:
        分类结果，包含每层命中统计和最终分类建议

    Raises:
        HTTPException 400: 文件列表为空
        HTTPException 500: 分类引擎内部错误
    """
    if not files:
        raise HTTPException(status_code=400, detail="文件列表不能为空")
    # 业务逻辑
    return ClassifyResponse(...)
```

### 命名约定
| 类型 | 约定 | 示例 |
|------|------|------|
| 类 | PascalCase | `Classifier`, `RagEngine` |
| 函数/方法 | snake_case | `classify_file`, `build_index` |
| 变量 | snake_case | `file_path`, `embedding_dim` |
| 常量 | UPPER_SNAKE_CASE | `MAX_CONTENT_LENGTH` |
| Pydantic 模型 | PascalCase + 语义后缀 | `ClassifyRequest`, `ChatResponse` |
| 私有成员 | _前缀 | `self._cache` |
| 模块文件 | snake_case | `ollama_client.py` |

### FastAPI 路由规范
```python
# api/routes_classify.py
from fastapi import APIRouter, HTTPException, Depends

router = APIRouter(prefix="/classify", tags=["分类"])

@router.post("", response_model=ClassifyResponse)
async def classify_files(
    request: ClassifyRequest,
    classifier: Classifier = Depends(get_classifier),
) -> ClassifyResponse:
    """对文件列表执行三层分类。"""
    if not request.files:
        raise HTTPException(status_code=400, detail="文件列表不能为空")
    result = await classifier.classify(request.files)
    return ClassifyResponse(items=result.items, stats=result.stats)
```

### 规范要点
- **异步优先**：所有 IO 操作（HTTP、数据库、文件）用 `async def`
- **Pydantic 校验**：所有请求/响应使用 Pydantic 模型
- **Docstring**：公共函数必须有 Google 风格 docstring（Args/Returns/Raises）
- **依赖注入**：用 FastAPI `Depends` 注入服务实例，不在路由内直接 `new`

### ruff + mypy 配置
```toml
# pyproject.toml
[tool.ruff]
target-version = "py312"
line-length = 100

[tool.ruff.lint]
select = ["E", "W", "F", "I", "N", "UP", "B", "A", "SIM", "TCH"]
ignore = ["E501"]

[tool.mypy]
python_version = "3.12"
strict = true
warn_return_any = true
disallow_untyped_defs = true
disallow_any_explicit = true
plugins = ["pydantic.mypy"]
```
