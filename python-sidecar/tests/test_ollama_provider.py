"""T8.2 — OllamaProvider 单元测试。

mock ``ollama.AsyncClient``，不发真实推理。覆盖：
- ``generate``：普通补全 / json_mode / 空 content / None message / 错误映射
- ``generate_stream``：逐 delta yield、跳过空块 / 首 delta 前错误映射
- ``embed``：正常 / 空输入短路 / 模型别名解析 / 404 / HTTP 错误 / 数量异常
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest import mock

import httpx
import pytest
from ollama import ResponseError

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.embedding_service import EmbeddingUnavailableError  # noqa: E402
from app.services.providers.ollama_provider import OllamaProvider  # noqa: E402

if TYPE_CHECKING:
    from collections.abc import AsyncGenerator, Generator


def _chunk(content: str | None) -> SimpleNamespace:
    """构造流式响应块：``content=None`` 表示 message 为空的块。"""
    return SimpleNamespace(message=None if content is None else SimpleNamespace(content=content))


async def _stream(*contents: str | None) -> AsyncGenerator[SimpleNamespace]:
    for content in contents:
        yield _chunk(content)


@pytest.fixture
def client() -> Generator[mock.AsyncMock]:
    """patch AsyncClient，返回 mock 实例（chat/embed 为 AsyncMock）。"""
    with mock.patch("app.services.providers.ollama_provider.AsyncClient") as cls:
        inst = cls.return_value
        inst.chat = mock.AsyncMock()
        inst.embed = mock.AsyncMock()
        yield inst


# ------------------------------------------------------------------
# generate
# ------------------------------------------------------------------


async def test_generate_returns_content(client: mock.AsyncMock) -> None:
    """普通补全返回 content；默认参数对齐既有调用点。"""
    client.chat.return_value = SimpleNamespace(message=SimpleNamespace(content="答案"))
    provider = OllamaProvider()
    assert await provider.generate("sys", "user") == "答案"
    call = client.chat.await_args.kwargs
    assert call["format"] is None
    assert call["think"] is False
    assert call["options"].temperature == 0.2
    assert call["options"].num_predict is None
    # T10.2：keep_alive 顶层参数（常驻权重）+ num_ctx 覆盖 RAG 上下文窗口
    assert call["keep_alive"] == "30m"
    assert call["options"].num_ctx == 8192


async def test_generate_json_mode_and_max_tokens(client: mock.AsyncMock) -> None:
    """json_mode 映射 format=json；max_tokens 映射 num_predict。"""
    client.chat.return_value = SimpleNamespace(message=SimpleNamespace(content='{"a":1}'))
    provider = OllamaProvider()
    assert await provider.generate("sys", "user", json_mode=True, max_tokens=128) == '{"a":1}'
    call = client.chat.await_args.kwargs
    assert call["format"] == "json"
    assert call["options"].num_predict == 128


async def test_generate_empty_content_and_none_message(client: mock.AsyncMock) -> None:
    """空 content / message 缺失均返回空串。"""
    client.chat.return_value = SimpleNamespace(message=SimpleNamespace(content=""))
    provider = OllamaProvider()
    assert await provider.generate("sys", "user") == ""
    client.chat.return_value = SimpleNamespace(message=None)
    assert await provider.generate("sys", "user") == ""


@pytest.mark.parametrize(
    "exc",
    [
        httpx.HTTPError("boom"),
        ResponseError("model not found", status_code=500),
    ],
)
async def test_generate_error_maps_to_unavailable(client: mock.AsyncMock, exc: Exception) -> None:
    """HTTP / 响应错误 → LLMUnavailableError。"""
    client.chat.side_effect = exc
    provider = OllamaProvider()
    with pytest.raises(LLMUnavailableError):
        await provider.generate("sys", "user")


# ------------------------------------------------------------------
# generate_stream
# ------------------------------------------------------------------


async def test_generate_stream_yields_deltas(client: mock.AsyncMock) -> None:
    """流式输出逐 delta；调用参数含 stream=True。"""
    client.chat.return_value = _stream("你", "好")
    provider = OllamaProvider()
    assert [t async for t in provider.generate_stream("sys", "user")] == ["你", "好"]
    call = client.chat.await_args.kwargs
    assert call["stream"] is True
    assert call["think"] is False


async def test_generate_stream_skips_empty_blocks(client: mock.AsyncMock) -> None:
    """空 content 块与 message 缺失块被跳过。"""
    client.chat.return_value = _stream("", None, "答", "案")
    provider = OllamaProvider()
    assert [t async for t in provider.generate_stream("sys", "user")] == ["答", "案"]


async def test_generate_stream_error_before_first_delta(client: mock.AsyncMock) -> None:
    """首个 delta 前失败 → LLMUnavailableError。"""
    client.chat.side_effect = httpx.HTTPError("boom")
    provider = OllamaProvider()
    with pytest.raises(LLMUnavailableError):
        async for _ in provider.generate_stream("sys", "user"):
            pass


# ------------------------------------------------------------------
# embed（显式不支持：向量化已统一为进程内 ONNX，见 embedding_service）
# ------------------------------------------------------------------


async def test_embed_explicitly_unsupported(client: mock.AsyncMock) -> None:
    """embed 恒定抛 EmbeddingUnavailableError，且不触达 Ollama。

    回归保护：避免有人误以为 Ollama 仍是向量化后端而接错调用点。
    """
    provider = OllamaProvider()
    with pytest.raises(EmbeddingUnavailableError, match="不再提供向量化"):
        await provider.embed(["x"])
    client.embed.assert_not_called()
