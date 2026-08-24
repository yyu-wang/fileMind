"""T8.1 — LLMProvider 抽象接口契约测试。

验证接口定义本身（三个抽象方法必需、不可直接实例化、mock 子类契约行为），
不涉及任何真实后端；具体 Provider 实现由 T8.2–T8.4 单独测试。
"""

from __future__ import annotations

import asyncio
import inspect
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.cloud_provider import LLMProvider  # noqa: E402

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

_ABSTRACT = {"generate", "generate_stream", "embed"}


def _make_subclass(*implemented: str) -> type[LLMProvider]:
    """构造仅实现指定抽象方法的子类（其余保持抽象）。"""

    async def _generate(
        self: LLMProvider,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        json_mode: bool = False,
        **kwargs: object,
    ) -> str:
        return "dummy"

    async def _generate_stream(
        self: LLMProvider,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        **kwargs: object,
    ) -> AsyncIterator[str]:
        yield "dummy"

    async def _embed(self: LLMProvider, texts: list[str], **kwargs: object) -> list[list[float]]:
        return [[] for _ in texts]

    impls: dict[str, object] = {
        "generate": _generate,
        "generate_stream": _generate_stream,
        "embed": _embed,
    }
    namespace = {name: impls[name] for name in implemented}
    return type("PartialProvider", (LLMProvider,), namespace)


# ------------------------------------------------------------------
# 接口定义
# ------------------------------------------------------------------


def test_abstract_methods_exact_set() -> None:
    """三个抽象方法齐全，无多余、无缺失。"""
    assert set(LLMProvider.__abstractmethods__) == _ABSTRACT


def test_llm_provider_not_directly_instantiable() -> None:
    """ABC 不能直接实例化。"""
    with pytest.raises(TypeError, match="abstract"):
        LLMProvider()  # type: ignore[abstract]


def test_subclass_missing_any_abstract_method_uninstantiable() -> None:
    """缺任意一个抽象方法的子类都无法实例化。"""
    for missing in _ABSTRACT:
        partial = _make_subclass(*(_ABSTRACT - {missing}))
        with pytest.raises(TypeError, match="abstract"):
            partial()  # type() 构建的动态子类，mypy 不跟踪其抽象方法状态


def test_generate_stream_is_async_generator() -> None:
    """流式方法必须是 async 生成器形态（对齐 generation_service.stream_generate）。"""
    cls = _make_subclass(*_ABSTRACT)
    assert inspect.isasyncgenfunction(cls.generate_stream)
    assert asyncio.iscoroutinefunction(cls.generate)
    assert asyncio.iscoroutinefunction(cls.embed)


# ------------------------------------------------------------------
# 契约行为（全实现 mock 子类）
# ------------------------------------------------------------------


def test_full_subclass_instantiable() -> None:
    """实现全部抽象方法后可实例化。"""
    provider = _make_subclass(*_ABSTRACT)()
    assert isinstance(provider, LLMProvider)


async def test_generate_returns_text_and_passes_kwargs() -> None:
    """非流式补全返回完整文本，kwargs 可透传（温度/上限/JSON 模式）。"""
    provider = _make_subclass(*_ABSTRACT)()
    assert await provider.generate("sys", "user") == "dummy"
    assert (
        await provider.generate("sys", "user", temperature=0.0, max_tokens=10, format="json")
        == "dummy"
    )


async def test_generate_stream_yields_tokens() -> None:
    """流式补全逐 token 产出。"""
    provider = _make_subclass(*_ABSTRACT)()
    assert [tok async for tok in provider.generate_stream("sys", "user")] == ["dummy"]


async def test_embed_returns_vector_per_input() -> None:
    """向量化返回与输入等长的向量列表，空输入返回空列表。"""
    provider = _make_subclass(*_ABSTRACT)()
    assert await provider.embed(["a", "b"]) == [[], []]
    assert await provider.embed([]) == []
