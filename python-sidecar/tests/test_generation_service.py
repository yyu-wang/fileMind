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
    from collections.abc import AsyncIterator, Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.generation_service import (  # noqa: E402
    CONTENT_MAX,
    SourceChunk,
    build_rag_prompt,
    reset_clients,
    stream_generate,
    stream_with_citations,
)


@pytest.fixture(autouse=True)
def _reset_clients() -> Generator[None, None, None]:
    """每个用例前后重置模块级客户端单例（避免跨用例复用上例的 fake）。"""
    reset_clients()
    yield
    reset_clients()


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
    # T10.2：keep_alive 顶层参数（常驻权重）+ num_ctx 覆盖 RAG 上下文窗口
    assert captured["keep_alive"] == "30m"
    assert captured["options"].num_ctx == 8192  # type: ignore[attr-defined]


async def test_stream_generate_reuses_single_client() -> None:
    """模块级单例：多次生成只构造一次 AsyncClient（httpx 连接池复用）。"""

    async def fake_chat(**kwargs: object) -> object:
        async def gen() -> object:
            yield SimpleNamespace(message=SimpleNamespace(content="ok"))

        return gen()

    client = _fake_client(fake_chat)
    with mock.patch("app.services.generation_service.AsyncClient", return_value=client) as patched:
        got1 = [tok async for tok in stream_generate("sys", "user")]
        got2 = [tok async for tok in stream_generate("sys", "user")]
    assert got1 == ["ok"]
    assert got2 == ["ok"]
    assert patched.call_count == 1


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


# ------------------------------------------------------------------
# T8.5 云端变体（Prompt 版本适配）
# ------------------------------------------------------------------


def test_build_prompt_cloud_variant_consistent_with_local() -> None:
    """云端变体输出格式约束与本地一致（引用标注 + 禁 JSON），仅精简指令/省 few-shot。"""
    local_system, local_user = build_rag_prompt("查询", [chunk(1)])
    cloud_system, cloud_user = build_rag_prompt("查询", [chunk(1)], version="cloud")
    # 输出格式 DoD：引用标注与禁 JSON 约束两者都有
    for template in (local_system, cloud_system):
        assert "[引用编号]" in template
        assert "不要输出 JSON" in template
    # 云端精简：省略 few-shot 示例（本地含示例正文，云端无该段）
    assert "根据财务报告" in local_user
    assert "根据财务报告" not in cloud_user


def test_build_prompt_cloud_truncates_context_by_model() -> None:
    """传入 model → 检索上下文按模型窗口截断（Token 长度适配）。"""
    # SC-m7：truncate_context 按 4 chars/token 换算，需更多片段才超窗
    # qwen3.8-27b: (32000-2000)*4 = 120000 chars；300 片段 * 500 = 150000 > 120000
    many_chunks = [chunk(i, text="x" * 500) for i in range(1, 301)]
    _, cloud_user = build_rag_prompt("查询", many_chunks, version="cloud", model="qwen3.8-27b")
    assert "[上下文已截断]" in cloud_user
    # 大窗口模型不触发截断（gpt-4o: (128000-2000)*4 = 504000 chars）
    _, local_user = build_rag_prompt("查询", many_chunks, model="gpt-4o")
    assert "[上下文已截断]" not in local_user


async def test_stream_generate_cloud_provider_delegates() -> None:
    """传入云端 Provider → 委托 generate_stream，不碰本地 AsyncClient。"""
    from app.rules.llm_classify import LLMUnavailableError

    class FakeCloud:
        version = "cloud"
        calls: list[tuple[str, str, float]] = []

        async def generate_stream(
            self,
            system: str,
            user: str,
            *,
            temperature: float = 0.2,
            max_tokens: int | None = None,
            **kwargs: object,
        ) -> AsyncIterator[str]:
            FakeCloud.calls.append((system, user, temperature))
            for delta in ["云端", "回答"]:
                yield delta

    got = [tok async for tok in stream_generate("sys", "user", provider=FakeCloud())]  # type: ignore[arg-type]
    assert got == ["云端", "回答"]
    assert len(FakeCloud.calls) == 1
    assert FakeCloud.calls[0][2] == 0.2

    class BoomCloud:
        version = "cloud"

        async def generate_stream(
            self,
            system: str,
            user: str,
            *,
            temperature: float = 0.2,
            max_tokens: int | None = None,
            **kwargs: object,
        ) -> AsyncIterator[str]:
            raise LLMUnavailableError("proxy down")
            yield ""  # pragma: no cover — 使函数成为 async 生成器

    with pytest.raises(LLMUnavailableError):
        async for _ in stream_generate("sys", "user", provider=BoomCloud()):  # type: ignore[arg-type]
            pass
