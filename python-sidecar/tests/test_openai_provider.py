"""T8.3 — OpenAIProvider 单元测试。

mock ``openai.AsyncOpenAI``，不发真实网络。覆盖：
- 构造：base_url / dummy api_key / token 头 / 模型名 / 超时重试
- ``generate``：普通补全 / json_mode 映射 response_format / 空 choices / 错误映射
- ``generate_stream``：逐 delta yield、跳过空块 / 首 delta 前错误
- ``embed``：始终抛 CloudUnavailableError
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest import mock

import httpx2
import openai
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.providers.openai_provider import (  # noqa: E402
    CLOUD_PROXY_BASE_URL,
    CloudUnavailableError,
    OpenAIProvider,
    _get_cached_client,
)

if TYPE_CHECKING:
    from collections.abc import AsyncGenerator, Generator


def _chunk(content: str | None = None) -> SimpleNamespace:
    """流式块：``content=None`` 表示 delta 缺失。"""
    return SimpleNamespace(choices=[SimpleNamespace(delta=SimpleNamespace(content=content))])


def _empty_chunk() -> SimpleNamespace:
    """无 choices 的空块（如仅含 usage 的终止块）。"""
    return SimpleNamespace(choices=[])


def _null_delta_chunk() -> SimpleNamespace:
    """choices 存在但 delta 为 None 的块。"""
    return SimpleNamespace(choices=[SimpleNamespace(delta=None)])


async def _stream(*chunks: SimpleNamespace) -> AsyncGenerator[SimpleNamespace]:
    for chunk in chunks:
        yield chunk


@pytest.fixture
def client() -> Generator[mock.MagicMock]:
    """patch AsyncOpenAI，返回 mock 类（构造参数记于 ``call_args``，create 记于子 mock）。"""
    # SC-m11：清 lru_cache 防跨测试拿到旧 mock 实例
    _get_cached_client.cache_clear()
    # P2-1：AsyncOpenAI 已惰性化（模块级不导入），patch 源模块；
    # ``_get_cached_client`` 调用时的 ``from openai import AsyncOpenAI`` 会取到该 mock。
    with mock.patch("openai.AsyncOpenAI") as cls:
        cls.return_value.chat.completions.create = mock.AsyncMock()
        yield cls
    _get_cached_client.cache_clear()


# ------------------------------------------------------------------
# 构造 / 配置
# ------------------------------------------------------------------


def test_default_base_url_dummy_key_token_header(client: mock.MagicMock) -> None:
    """默认 base_url 指向本地代理 openai 端点；api_key 为占位；token 头取自 env。"""
    with mock.patch.dict("os.environ", {"FILEMIND_CLOUD_PROXY_TOKEN": "tok-123"}):
        OpenAIProvider()
    kwargs = client.call_args.kwargs
    assert kwargs["base_url"] == f"{CLOUD_PROXY_BASE_URL}/cloud-proxy/openai"
    assert kwargs["api_key"] == "filemind-proxy"
    assert kwargs["default_headers"]["x-filemind-token"] == "tok-123"


def test_custom_base_url_model_and_token(client: mock.MagicMock) -> None:
    """显式传 base_url / model / token 生效。"""
    OpenAIProvider(base_url="http://x/cloud-proxy/deepseek", model="deepseek-chat", token="t2")
    kwargs = client.call_args.kwargs
    assert kwargs["base_url"] == "http://x/cloud-proxy/deepseek"
    assert kwargs["default_headers"]["x-filemind-token"] == "t2"
    assert kwargs["timeout"] == 120.0
    assert kwargs["max_retries"] == 1


# ------------------------------------------------------------------
# generate
# ------------------------------------------------------------------


async def test_generate_returns_content(client: mock.MagicMock) -> None:
    """普通补全返回 content；默认参数对齐接口契约。"""
    client.return_value.chat.completions.create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content="云端答案"))]
    )
    provider = OpenAIProvider()
    assert await provider.generate("sys", "user") == "云端答案"
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["model"] == "gpt-4o"
    assert call["temperature"] == 0.2
    assert call["max_tokens"] is None
    # 非 json_mode 传 Omit 哨兵（SDK 运行时将其从请求体剥离，等价不传）
    assert isinstance(call["response_format"], openai.Omit)
    assert call["messages"][0]["role"] == "system"
    assert call["messages"][1]["role"] == "user"


async def test_generate_json_mode_maps_response_format(client: mock.MagicMock) -> None:
    """json_mode → response_format json_object；max_tokens 透传。"""
    client.return_value.chat.completions.create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content='{"ok":1}'))]
    )
    provider = OpenAIProvider()
    assert await provider.generate("sys", "user", json_mode=True, max_tokens=128) == '{"ok":1}'
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["response_format"] == {"type": "json_object"}
    assert call["max_tokens"] == 128


async def test_generate_no_choices_returns_empty(client: mock.MagicMock) -> None:
    """choices 为空或 message.content 非 str 时返回空串。"""
    create = client.return_value.chat.completions.create
    provider = OpenAIProvider()
    create.return_value = SimpleNamespace(choices=[])
    assert await provider.generate("sys", "user") == ""
    create.return_value = SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content=None))]
    )
    assert await provider.generate("sys", "user") == ""


@pytest.mark.parametrize(
    "exc",
    [
        openai.OpenAIError("boom"),  # 基类（SDK 全部错误均继承自它）
        openai.APIConnectionError(request=httpx2.Request("POST", "http://x")),  # 网络子类
    ],
)
async def test_generate_error_maps_to_cloud_unavailable(
    client: mock.MagicMock, exc: Exception
) -> None:
    """SDK 错误（网络 / 通用）→ CloudUnavailableError。"""
    client.return_value.chat.completions.create.side_effect = exc
    provider = OpenAIProvider()
    with pytest.raises(CloudUnavailableError):
        await provider.generate("sys", "user")


# ------------------------------------------------------------------
# generate_stream
# ------------------------------------------------------------------


async def test_generate_stream_yields_deltas(client: mock.MagicMock) -> None:
    """流式输出逐 delta；请求含 stream=True。"""
    client.return_value.chat.completions.create.return_value = _stream(
        _chunk("你"), _chunk("好"), _empty_chunk(), _chunk("!")
    )
    provider = OpenAIProvider()
    assert [t async for t in provider.generate_stream("sys", "user")] == ["你", "好", "!"]
    call = client.return_value.chat.completions.create.await_args.kwargs
    assert call["stream"] is True


async def test_generate_stream_skips_empty_and_null_delta(
    client: mock.MagicMock,
) -> None:
    """空块 / None delta 跳过，不影响后续输出。"""
    client.return_value.chat.completions.create.return_value = _stream(
        _empty_chunk(), _chunk(""), _null_delta_chunk(), _chunk("答"), _chunk("案")
    )
    provider = OpenAIProvider()
    assert [t async for t in provider.generate_stream("sys", "user")] == ["答", "案"]


async def test_generate_stream_error_before_first_delta(client: mock.MagicMock) -> None:
    """首个 delta 前失败 → CloudUnavailableError。"""
    client.return_value.chat.completions.create.side_effect = openai.OpenAIError("boom")
    provider = OpenAIProvider()
    with pytest.raises(CloudUnavailableError):
        async for _ in provider.generate_stream("sys", "user"):
            pass


# ------------------------------------------------------------------
# embed
# ------------------------------------------------------------------


async def test_embed_raises_cloud_unavailable(client: mock.MagicMock) -> None:
    """云端 Embedding 未启用，任何输入均抛 CloudUnavailableError。"""
    provider = OpenAIProvider()
    with pytest.raises(CloudUnavailableError, match="云端 Embedding 未启用"):
        await provider.embed(["a", "b"])
    client.return_value.chat.completions.create.assert_not_awaited()
