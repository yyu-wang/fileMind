"""T8.4 — DeepSeekProvider 单元测试。

mock ``AsyncOpenAI``（复用父类 patch 点），不发真实网络。覆盖：
- 构造：base_url 指向 deepseek 段 / 默认模型 deepseek-chat / token 头
- 中文优化：默认 max_tokens=2048（传 None 时生效，显式覆盖则用显式值）
- 继承行为：generate / generate_stream / json_mode / 错误映射
- ``embed``：明确报错（DeepSeek 无 Embedding API）
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest import mock

import openai
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.providers.deepseek_provider import (  # noqa: E402
    _DEFAULT_MAX_TOKENS,
    DEEPSEEK_MODEL,
    DeepSeekProvider,
)
from app.services.providers.openai_provider import (  # noqa: E402
    CLOUD_PROXY_BASE_URL,
    CloudUnavailableError,
    OpenAIProvider,
)

if TYPE_CHECKING:
    from collections.abc import AsyncGenerator, Generator


def _chunk(content: str | None = None) -> SimpleNamespace:
    """流式块：``content=None`` 表示 delta 缺失。"""
    return SimpleNamespace(choices=[SimpleNamespace(delta=SimpleNamespace(content=content))])


async def _stream(*chunks: SimpleNamespace) -> AsyncGenerator[SimpleNamespace]:
    for chunk in chunks:
        yield chunk


@pytest.fixture
def client() -> Generator[mock.MagicMock]:
    """patch AsyncOpenAI（DeepSeekProvider 构造经父类调用它）。"""
    # SC-m11：清 lru_cache 防跨测试拿到旧 mock 实例
    from app.services.providers.openai_provider import _get_cached_client

    _get_cached_client.cache_clear()
    with mock.patch("app.services.providers.openai_provider.AsyncOpenAI") as cls:
        cls.return_value.chat.completions.create = mock.AsyncMock()
        yield cls
    _get_cached_client.cache_clear()


# ------------------------------------------------------------------
# 构造 / 配置
# ------------------------------------------------------------------


def test_default_base_url_model_and_token(client: mock.MagicMock) -> None:
    """默认 base_url 指向代理 deepseek 段；模型 deepseek-chat；token 头取自 env。"""
    with mock.patch.dict("os.environ", {"FILEMIND_CLOUD_PROXY_TOKEN": "tok-ds"}):
        DeepSeekProvider()
    kwargs = client.call_args.kwargs
    assert kwargs["base_url"] == f"{CLOUD_PROXY_BASE_URL}/cloud-proxy/deepseek"
    assert kwargs["default_headers"]["x-filemind-token"] == "tok-ds"
    assert DEEPSEEK_MODEL == "deepseek-chat"


def test_is_openai_provider_subclass() -> None:
    """DeepSeekProvider 是 OpenAIProvider 子类（复用 OpenAI 兼容格式）。"""
    assert issubclass(DeepSeekProvider, OpenAIProvider)


# ------------------------------------------------------------------
# generate（中文优化参数）
# ------------------------------------------------------------------


async def test_generate_uses_default_max_tokens_when_none(client: mock.MagicMock) -> None:
    """未显式指定 max_tokens 时回落 2048（中文长回答防截断）。"""
    client.return_value.chat.completions.create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content="答案"))]
    )
    provider = DeepSeekProvider()
    assert await provider.generate("sys", "user") == "答案"
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["model"] == "deepseek-chat"
    assert call["max_tokens"] == _DEFAULT_MAX_TOKENS


async def test_generate_explicit_max_tokens_overrides_default(
    client: mock.MagicMock,
) -> None:
    """显式 max_tokens 覆盖中文默认。"""
    client.return_value.chat.completions.create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content="x"))]
    )
    provider = DeepSeekProvider()
    await provider.generate("sys", "user", max_tokens=128)
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["max_tokens"] == 128


async def test_generate_json_mode_maps_response_format(client: mock.MagicMock) -> None:
    """json_mode → response_format json_object（继承自 OpenAIProvider）。"""
    client.return_value.chat.completions.create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content='{"a":1}'))]
    )
    provider = DeepSeekProvider()
    assert await provider.generate("sys", "user", json_mode=True) == '{"a":1}'
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["response_format"] == {"type": "json_object"}


async def test_generate_error_maps_to_cloud_unavailable(client: mock.MagicMock) -> None:
    """SDK 错误 → CloudUnavailableError（继承自 OpenAIProvider）。"""
    client.return_value.chat.completions.create.side_effect = openai.OpenAIError("boom")
    provider = DeepSeekProvider()
    with pytest.raises(CloudUnavailableError):
        await provider.generate("sys", "user")


# ------------------------------------------------------------------
# generate_stream（中文优化参数）
# ------------------------------------------------------------------


async def test_generate_stream_yields_deltas(client: mock.MagicMock) -> None:
    """流式输出逐 delta；请求含 stream=True 与默认 max_tokens。"""
    client.return_value.chat.completions.create.return_value = _stream(_chunk("你"), _chunk("好"))
    provider = DeepSeekProvider()
    assert [t async for t in provider.generate_stream("sys", "user")] == ["你", "好"]
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["stream"] is True
    assert call["max_tokens"] == _DEFAULT_MAX_TOKENS


# ------------------------------------------------------------------
# embed
# ------------------------------------------------------------------


async def test_embed_raises_deepseek_not_supported(client: mock.MagicMock) -> None:
    """DeepSeek 无 Embedding API，任何输入均抛 CloudUnavailableError。"""
    provider = DeepSeekProvider()
    with pytest.raises(CloudUnavailableError, match="DeepSeek 未提供 Embedding API"):
        await provider.embed(["a"])
    client.return_value.chat.completions.create.assert_not_awaited()
