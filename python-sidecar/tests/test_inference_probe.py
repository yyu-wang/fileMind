"""T6.7 — services.inference_probe_service 单元测试。

覆盖：Ollama 可用（LLM 过滤 + Embedding 别名匹配 + :latest 剥离）、连接失败、
响应结构异常（mock httpx，不走真实 Ollama）。pyproject 配 asyncio_mode=auto，
async 测试函数自动运行。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

import httpx

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.inference_probe_service import probe_ollama  # noqa: E402


class _FakeResponse:
    """模拟 httpx.Response（只带 raise_for_status + json）。"""

    def __init__(self, payload: dict[str, object]) -> None:
        self._payload = payload

    def raise_for_status(self) -> None:
        return None

    def json(self) -> dict[str, object]:
        return self._payload


def _fake_client(payload: dict[str, object]) -> mock.AsyncMock:
    """构造返回固定 /api/tags payload 的 fake AsyncClient（async 上下文）。"""
    client = mock.AsyncMock()
    client.get.return_value = _FakeResponse(payload)
    client.__aenter__.return_value = client
    client.__aexit__.return_value = False
    return client


TAGS_PAYLOAD: dict[str, object] = {
    "models": [
        {
            "name": "qwen3.8-27b",
            "size": 16_000_000_000,
            "modified_at": "2026-08-01T00:00:00Z",
            "details": {"family": "qwen3"},
        },
        {
            "name": "qllama/bge-large-zh-v1.5",
            "size": 1_300_000_000,
            "modified_at": "2026-08-02T00:00:00Z",
            "details": {"family": "bert"},
        },
        {
            "name": "bge-small-zh-v1.5:latest",
            "size": 90_000_000,
            "modified_at": "2026-08-03T00:00:00Z",
            "details": {"family": "bert"},
        },
    ]
}


async def test_probe_success_filters_llm_and_matches_embeddings() -> None:
    """可用：LLM 排除 embedding 名/别名，Embedding 按别名匹配可用性。"""
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client(TAGS_PAYLOAD),
    ):
        result = await probe_ollama()

    assert result.available is True
    assert result.status == "ok"
    assert result.error_code is None

    # LLM 列表只含生成模型，embedding 注册表名与别名被排除；:latest 已剥离
    assert [m.name for m in result.llm_models] == ["qwen3.8-27b"]
    assert result.llm_models[0].size_bytes == 16_000_000_000
    assert result.llm_models[0].family == "qwen3"

    # Embedding 可用性：别名安装判定 + 维度/版本透传
    by_name = {m.name: m for m in result.embedding_models}
    assert by_name["bge-large-zh-v1.5"].available is True
    assert by_name["bge-small-zh-v1.5"].available is True
    assert by_name["bge-m3"].available is False
    assert by_name["bge-large-zh-v1.5"].dim == 1024
    assert by_name["bge-large-zh-v1.5"].version == 1


async def test_probe_missing_details_field() -> None:
    """details 字段缺失 → family=None，不抛错。"""
    payload: dict[str, object] = {"models": [{"name": "qwen3.8-27b", "size": 100}]}
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client(payload),
    ):
        result = await probe_ollama()

    assert result.available is True
    assert len(result.llm_models) == 1
    assert result.llm_models[0].family is None
    assert result.llm_models[0].modified_at is None


async def test_probe_conn_error_returns_unavailable() -> None:
    """连接失败 → available=false + OLLAMA_UNAVAILABLE，Embedding 全不可用。"""
    client = mock.AsyncMock()
    client.get.side_effect = httpx.ConnectError("connection refused")
    client.__aenter__.return_value = client
    client.__aexit__.return_value = False
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=client,
    ):
        result = await probe_ollama()

    assert result.available is False
    assert result.status == "unavailable"
    assert result.error_code == "OLLAMA_UNAVAILABLE"
    assert result.llm_models == []
    assert len(result.embedding_models) == 3
    assert all(not m.available for m in result.embedding_models)


async def test_probe_malformed_response_returns_unavailable() -> None:
    """/api/tags 响应结构异常 → ValidationError → available=false，不 5xx。"""
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client({"models": "boom"}),
    ):
        result = await probe_ollama()

    assert result.available is False
    assert result.status == "unavailable"
    assert result.error_code == "OLLAMA_UNAVAILABLE"
