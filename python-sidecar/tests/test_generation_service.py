"""T5.6 — RAG 生成服务单元测试（P-03 提示词 + 流式生成 + 引用解析）。

覆盖：P-03 提示词构建（SYSTEM 回答规则 / USER 片段与 Few-shot / 500 截断）、
``stream_with_citations`` 引用解析（内联标记顺序 / 幻觉引用丢弃 / 残留文本 /
跨 token 拆分 / 连续标记）、``stream_generate``（mock AsyncClient 流式 delta，
连接失败抛 LLMUnavailableError）。均不发真实推理。
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest import mock

import httpx
import pytest

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.generation_service import (  # noqa: E402
    CONTENT_MAX,
    SourceChunk,
    build_rag_prompt,
    stream_generate,
    stream_with_citations,
)


def chunk(citation_id: int, text: str = "2024年Q3营收为5.2亿元", page: int = 3) -> SourceChunk:
    """构造一条检索片段。"""
    return SourceChunk(
        citation_id=citation_id,
        file_name="2024Q3财务报告.pdf",
        page=page,
        text=text,
    )


# ------------------------------------------------------------------
# build_rag_prompt（P-03）
# ------------------------------------------------------------------


def test_system_has_answer_rules_and_citation_format() -> None:
    """SYSTEM 含 6 条回答规则与引用标注约束，明确「不要输出 JSON」。"""
    system, _ = build_rag_prompt("查询", [chunk(1)])
    assert "回答规则" in system
    assert "引用来源" in system and "[引用编号]" in system
    assert "未找到相关信息" in system
    assert "不要输出 JSON" in system


def test_user_has_query_blocks_fewshot_and_pending() -> None:
    """USER 含用户问题、[N] 来源…内容…、Few-shot 示例与 [待回答]。"""
    _, user = build_rag_prompt("2024年Q3营收多少？", [chunk(1), chunk(2, "营收同比增长15.6%")])
    assert "2024年Q3营收多少？" in user
    assert "[1] 来源：2024Q3财务报告.pdf（第 3 页）" in user
    assert "内容：2024年Q3营收为5.2亿元" in user
    assert "[2] 来源：2024Q3财务报告.pdf（第 3 页）" in user
    assert "示例" in user
    assert "[待回答]" in user


def test_user_truncates_chunk_text_to_500() -> None:
    """片段内容截断到 500 字符（对齐 P-03 输入变量 source_N_content）。"""
    long_text = "x" * (CONTENT_MAX + 100)
    _, user = build_rag_prompt("查询", [chunk(1, long_text)])
    assert "x" * CONTENT_MAX in user
    assert "x" * (CONTENT_MAX + 1) not in user


# ------------------------------------------------------------------
# stream_with_citations（引用解析）
# ------------------------------------------------------------------


async def _collect(tokens: list[str], valid_ids: set[int]) -> list[tuple[str, object]]:
    """把 token 流喂给 stream_with_citations，收集全部事件。"""

    async def token_stream() -> AsyncIterator[str]:
        for t in tokens:
            yield t

    return [item async for item in stream_with_citations(token_stream(), valid_ids)]


async def test_inline_markers_parse_in_order() -> None:
    """内联 [N] 标记 → 标记前文本为 text、标记为 citation，顺序保持。"""
    tokens = ["根据财务报告，2024年营收为5.2亿元", "[1]", "。同比增长", "[2]", "15.6%。"]
    events = await _collect(tokens, {1, 2})
    assert events == [
        ("text", "根据财务报告，2024年营收为5.2亿元"),
        ("citation", 1),
        ("text", "。同比增长"),
        ("citation", 2),
        ("text", "15.6%。"),
    ]


async def test_invalid_id_marker_dropped() -> None:
    """不在 valid_ids 的引用标记（幻觉引用）丢弃，仅保留正文。"""
    events = await _collect(["[99]", "残留文本"], {1, 2})
    assert events == [("text", "残留文本")]


async def test_trailing_buffer_flushed_as_text() -> None:
    """无标记的残留 buffer 流结束后作为 text 产出。"""
    events = await _collect(["纯文本回答，无引用"], {1})
    assert events == [("text", "纯文本回答，无引用")]


async def test_consecutive_markers() -> None:
    """连续 [1][2] → 两个 citation 事件，无空文本。"""
    events = await _collect(["[1]", "[2]", "结论"], {1, 2})
    assert events == [("citation", 1), ("citation", 2), ("text", "结论")]


async def test_marker_split_across_tokens() -> None:
    """标记被拆到相邻 token（流式分块）→ 仍能识别。"""
    events = await _collect(["根据报告", "[", "1]", "营收5.2亿元"], {1})
    assert events == [
        ("text", "根据报告"),
        ("citation", 1),
        ("text", "营收5.2亿元"),
    ]


# ------------------------------------------------------------------
# stream_generate（mock AsyncClient）
# ------------------------------------------------------------------


def _fake_client(chat_impl: object) -> mock.MagicMock:
    """构造 AsyncClient mock：``.chat`` 替换为给定 async 实现。"""
    client = mock.MagicMock()
    client.chat = chat_impl
    return client


async def test_stream_generate_yields_non_empty_deltas() -> None:
    """流式响应 → 跳过 None/空内容，yield 非空 delta；携带正确调用参数。"""
    captured: dict[str, object] = {}

    async def fake_chat(**kwargs: object) -> object:
        captured.update(kwargs)

        async def gen() -> object:
            for content in ["根据", "", "报告", None]:
                yield SimpleNamespace(message=SimpleNamespace(content=content))

        return gen()

    client = _fake_client(fake_chat)
    with mock.patch("app.services.generation_service.AsyncClient", return_value=client):
        got = [tok async for tok in stream_generate("sys", "user")]

    assert got == ["根据", "报告"]
    assert captured["stream"] is True
    assert captured["think"] is False
    assert captured["model"] == "qwen3.8-27b"
    assert captured["options"].temperature == 0.2  # type: ignore[attr-defined]


async def test_stream_generate_conn_error_raises_unavailable() -> None:
    """连接失败（httpx.HTTPError）→ LLMUnavailableError 在首个 delta 前抛出。"""

    async def raise_conn(**kwargs: object) -> object:
        raise httpx.ConnectError("connection refused")

    client = _fake_client(raise_conn)
    with (
        mock.patch("app.services.generation_service.AsyncClient", return_value=client),
        pytest.raises(LLMUnavailableError),
    ):
        async for _ in stream_generate("sys", "user"):
            pass
