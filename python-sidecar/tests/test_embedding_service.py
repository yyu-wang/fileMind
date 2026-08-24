"""T5.1 — services.embedding_service 单元测试。

覆盖：批量/单条向量生成、空输入、连接失败、模型未拉取（404）、HTTP 错误、
超时（mock 网络，不走真实 Ollama 请求）。pyproject 配 asyncio_mode=auto，
async 测试函数自动运行。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import httpx
import pytest
from ollama import ResponseError

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.embedding_service import (  # noqa: E402
    EMBEDDING_MODEL,
    EmbeddingUnavailableError,
    _ollama_model_name,
    embed_text,
    embed_texts,
    reset_clients,
)


@pytest.fixture(autouse=True)
def _reset_clients() -> Generator[None, None, None]:
    """每个用例前后重置模块级客户端单例（避免跨用例复用上例的 fake）。"""
    reset_clients()
    yield
    reset_clients()


class _FakeEmbedResponse:
    """模拟 ollama EmbedResponse（只带 .embeddings 字段）。"""

    def __init__(self, embeddings: list[list[float]]) -> None:
        self.embeddings = embeddings


def _fake_client(embeddings: list[list[float]]) -> mock.MagicMock:
    """构造返回固定向量的 fake AsyncClient。"""

    async def fake_embed(**kwargs: object) -> _FakeEmbedResponse:
        return _FakeEmbedResponse(embeddings)

    client = mock.MagicMock()
    client.embed = fake_embed
    return client


async def test_embed_texts_returns_vectors() -> None:
    """批量输入 → 等长向量列表，维度透传。"""
    embeddings = [[1.0, 2.0], [3.0, 4.0]]
    with mock.patch(
        "app.services.embedding_service.AsyncClient",
        return_value=_fake_client(embeddings),
    ):
        result = await embed_texts(["a", "b"])
    assert result == [[1.0, 2.0], [3.0, 4.0]]


async def test_embed_text_single() -> None:
    """单条文本 → 单个向量。"""
    embeddings = [[0.1, 0.2, 0.3]]
    with mock.patch(
        "app.services.embedding_service.AsyncClient",
        return_value=_fake_client(embeddings),
    ):
        result = await embed_text("hello")
    assert result == [0.1, 0.2, 0.3]


async def test_embed_texts_empty_input() -> None:
    """空列表 → 空列表，不发起请求。"""
    with mock.patch(
        "app.services.embedding_service.AsyncClient",
        return_value=_fake_client([]),
    ) as patched:
        result = await embed_texts([])
    assert result == []
    patched.assert_not_called()


async def test_embed_conn_error_raises_unavailable() -> None:
    """连接失败（httpx.ConnectError）→ EmbeddingUnavailableError。"""

    async def raise_conn(**kwargs: object) -> object:
        raise httpx.ConnectError("connection refused")

    client = mock.MagicMock()
    client.embed = raise_conn
    with (
        mock.patch("app.services.embedding_service.AsyncClient", return_value=client),
        pytest.raises(EmbeddingUnavailableError, match="Embedding 调用失败"),
    ):
        await embed_texts(["x"])


async def test_embed_model_not_pulled_404() -> None:
    """模型未拉取（Ollama 404）→ 消息提示 ollama pull。"""

    async def raise_404(**kwargs: object) -> object:
        raise ResponseError("model not found", status_code=404)

    client = mock.MagicMock()
    client.embed = raise_404
    with (
        mock.patch("app.services.embedding_service.AsyncClient", return_value=client),
        pytest.raises(EmbeddingUnavailableError, match="ollama pull"),
    ):
        await embed_texts(["x"])


async def test_embed_http_error_non_404() -> None:
    """非 404 的 Response 错误 → 统一 EmbeddingUnavailableError。"""

    async def raise_500(**kwargs: object) -> object:
        raise ResponseError("internal error", status_code=500)

    client = mock.MagicMock()
    client.embed = raise_500
    with (
        mock.patch("app.services.embedding_service.AsyncClient", return_value=client),
        pytest.raises(EmbeddingUnavailableError, match="Embedding 调用失败"),
    ):
        await embed_texts(["x"])


async def test_embed_timeout_raises_unavailable() -> None:
    """超时 → EmbeddingUnavailableError 含超时提示。"""

    async def slow(**kwargs: object) -> object:
        raise TimeoutError

    client = mock.MagicMock()
    client.embed = slow
    with (
        mock.patch("app.services.embedding_service.AsyncClient", return_value=client),
        pytest.raises(EmbeddingUnavailableError, match="超时"),
    ):
        await embed_texts(["x"])


async def test_embed_count_mismatch_raises() -> None:
    """返回向量数量与输入不一致 → EmbeddingUnavailableError。"""
    embeddings = [[1.0]]
    with (
        mock.patch(
            "app.services.embedding_service.AsyncClient",
            return_value=_fake_client(embeddings),
        ),
        pytest.raises(EmbeddingUnavailableError, match="数量异常"),
    ):
        await embed_texts(["a", "b"])


def test_ollama_model_alias_resolution() -> None:
    """注册表标识 → 社区命名空间；未知模型原样返回（支持 env 覆盖）。"""
    assert _ollama_model_name("bge-large-zh-v1.5") == "qllama/bge-large-zh-v1.5"
    assert _ollama_model_name("custom-model") == "custom-model"
    assert EMBEDDING_MODEL == "bge-large-zh-v1.5"


async def test_embed_sends_keep_alive() -> None:
    """embed 调用携带 keep_alive（模型常驻，避免重复问句二次冷加载）。"""
    captured: dict[str, object] = {}

    async def fake_embed(**kwargs: object) -> _FakeEmbedResponse:
        captured.update(kwargs)
        return _FakeEmbedResponse([[1.0]])

    client = mock.MagicMock()
    client.embed = fake_embed
    with mock.patch("app.services.embedding_service.AsyncClient", return_value=client):
        await embed_texts(["x"])

    assert captured["keep_alive"] == "30m"


async def test_embed_texts_reuses_single_client() -> None:
    """模块级单例：多次调用只构造一次 AsyncClient（httpx 连接池复用）。"""
    embeddings = [[1.0]]
    with mock.patch(
        "app.services.embedding_service.AsyncClient",
        return_value=_fake_client(embeddings),
    ) as patched:
        await embed_texts(["a"])
        await embed_texts(["b"])
    assert patched.call_count == 1
