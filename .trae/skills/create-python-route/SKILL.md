---
name: "create-python-route"
description: "Creates a new FastAPI route with Pydantic models, type annotations, docstring, and test scaffold. Invoke when adding a new Python Sidecar API endpoint."
---

# Create Python Route

Creates a new FastAPI route following project Python strict typing and documentation standards.

## When to Invoke

- User asks to create a new FastAPI endpoint
- User asks to add a new Python API route
- Adding a new Sidecar service endpoint

## Steps

### 1. Read Rules First

Before generating any code, read:
- `rules/python.md` — Python/FastAPI coding rules
- `rules/security.md` — Security rules (if endpoint handles sensitive data)

### 2. Determine Route Details

Ask or infer:
- Route path (e.g. `/classify`, `/chat/query`)
- HTTP method (GET/POST/PUT/DELETE)
- Request body model (Pydantic)
- Response model (Pydantic)
- Which service it depends on (Classifier, RagEngine, Indexer, etc.)
- Whether it needs streaming (SSE)

### 3. Generate Pydantic Models

Write to `python-sidecar/app/models/{module}.py`:

```python
from pydantic import BaseModel, Field


class {Entity}Request(BaseModel):
    """Request model for {operation}."""
    field1: str = Field(..., description="Description")
    field2: int = Field(default=100, ge=1, le=10000)


class {Entity}Response(BaseModel):
    """Response model for {operation}."""
    result: str
    count: int
    metadata: dict[str, str] = Field(default_factory=dict)
```

### 4. Generate Route File

Write to `python-sidecar/app/api/routes_{module}.py`:

```python
from fastapi import APIRouter, HTTPException, Depends

from app.models.{module} import {Entity}Request, {Entity}Response
from app.core.{service} import {ServiceClass}

router = APIRouter(prefix="/{module}", tags=["{标签}"])


@router.post("", response_model={Entity}Response)
async def {endpoint_name}(
    request: {Entity}Request,
    service: {ServiceClass} = Depends({ServiceClass}.get_instance),
) -> {Entity}Response:
    """{One-line description of what this endpoint does}.

    Args:
        request: {Description of request body}
        service: Injected service instance

    Returns:
        {Description of response}

    Raises:
        HTTPException 400: {When this happens}
        HTTPException 500: {When this happens}
    """
    if not request.field1:
        raise HTTPException(status_code=400, detail="field1 不能为空")

    try:
        result = await service.process(request)
        return {Entity}Response(result=result.value, count=result.count)
    except ServiceError as e:
        raise HTTPException(status_code=500, detail=str(e)) from e
```

### 5. Register Router

Add to `python-sidecar/app/main.py`:

```python
from app.api import routes_{module}

app.include_router(routes_{module}.router)
```

### 6. Code Quality Checklist (MUST verify)

- [ ] All function parameters have type annotations
- [ ] Return type annotated
- [ ] Google-style docstring with Args/Returns/Raises
- [ ] No `Any` type — use `object` or generics
- [ ] No `# type: ignore`
- [ ] Request/Response use Pydantic models (not raw dict)
- [ ] Async function (`async def`) for all IO operations
- [ ] Error handling with HTTPException (not bare exceptions)
- [ ] No `print()` — use `logging`

### 7. Generate Test Scaffold

Write to `python-sidecar/tests/test_{module}.py`:

```python
import pytest
from fastapi.testclient import TestClient

from app.main import app


@pytest.fixture
def client():
    return TestClient(app)


def test_{endpoint_name}_success(client):
    response = client.post("/{module}", json={"field1": "test", "field2": 100})
    assert response.status_code == 200
    data = response.json()
    assert "result" in data


def test_{endpoint_name}_empty_request(client):
    response = client.post("/{module}", json={"field1": "", "field2": 100})
    assert response.status_code == 400


def test_{endpoint_name}_invalid_field(client):
    response = client.post("/{module}", json={"field1": "test", "field2": 0})
    assert response.status_code == 422  # Pydantic validation
```

### 8. Self-Check

```bash
ruff check python-sidecar/app/api/routes_{module}.py
ruff format --check python-sidecar/app/api/routes_{module}.py
mypy python-sidecar/app/
pytest python-sidecar/tests/test_{module}.py
```

All four MUST pass.
