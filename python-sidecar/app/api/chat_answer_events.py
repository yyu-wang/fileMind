"""RAG 流式流水线的生成阶段：累积器 + token / citation / done 事件组装。

（2026-09-18 按阶段拆自 ``chat_pipeline.py``，310 行超 Python 警告阈值 300；
检索阶段的整形与 search 事件仍在 :mod:`app.api.chat_pipeline`，与编排侧的
``chat_retrieve`` / ``chat_answer`` 拆分对称。）

本模块只放**纯变换**（不发起 IO、不解析全局状态）：由 ``stream_with_citations``
的引用事件流累积答案文本、引用编号与 token 数，再组装 SSE 的 token / citation /
done 帧。``CitationEvent`` 与 ``RagEvent`` 分别为流入与流出的帧形状。
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from app.api.chat_pipeline import elapsed_ms
from app.core.logging import getLogger

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.api.chat_pipeline import RagEvent
    from app.models import ChatStreamRequest
    from app.services.cloud_provider import LLMProvider, PromptVersion
    from app.services.rag_prompt import SourceChunk

logger = getLogger("filemind.chat")

#: ``stream_with_citations`` 的产出：``("text", 正文)`` 或 ``("citation", 编号)``
CitationEvent = tuple[str, str | int]


@dataclass(frozen=True)
class AnswerContext:
    """P-03 生成与 P-04 自我纠正共享的只读上下文。"""

    request: ChatStreamRequest
    provider: LLMProvider | None
    rewritten_query: str
    chunks: list[SourceChunk]
    version: PromptVersion


@dataclass
class AnswerAccumulator:
    """生成阶段的可变累积状态（跨 P-04 重试共享 ``total_tokens``）。"""

    started: float
    total_tokens: int = 0
    parts: list[str] = field(default_factory=list)
    cited: list[int] = field(default_factory=list)
    first_token_ms: int | None = None
    low_confidence: bool = False
    failed: bool = False

    def begin_retry(self) -> None:
        """进入下一轮修正回答：重置本轮文本与引用（token 计数继续累计）。"""
        self.parts = []
        self.cited = []


def _estimate_tokens(text: str) -> int:
    """SC-m19：按字符数/4 估算 token（原 ``+= 1`` 统计的是块数非 token）。"""
    return max(1, len(text) // 4)


async def token_events(
    events: AsyncIterator[CitationEvent],
    acc: AnswerAccumulator,
    *,
    with_ttft: bool = False,
) -> AsyncIterator[RagEvent]:
    """把引用解析流转成 token 事件，并累积答案片段、引用编号与 token 数。

    Args:
        events: ``stream_with_citations`` 的输出流。
        acc: 累积器（答案片段 / 引用编号 / token 数就地更新）。
        with_ttft: 首个正文片段是否记录 ``first_token_ms`` 与 ``chat.first_token``
            日志（仅首轮生成开启；重试轮不再重复记录 TTFT）。

    Yields:
        ``("token", {"content": ...})`` 事件（与输入流的片段一一对应）。
    """
    async for event in events:
        if event[0] == "text":
            text = str(event[1])
            if with_ttft and acc.first_token_ms is None:
                acc.first_token_ms = elapsed_ms(acc.started)
                logger.info("chat.first_token", ttft_ms=acc.first_token_ms)
            acc.parts.append(text)
            acc.total_tokens += _estimate_tokens(text)
            yield ("token", {"content": text})
        else:
            citation_id = int(event[1])
            acc.cited.append(citation_id)
            acc.parts.append(f"[{citation_id}]")


def build_citations(cited: list[int], chunks: list[SourceChunk]) -> list[dict[str, object]]:
    """引用明细：去重 + 过滤幻觉编号 + 映射片段原文。

    去重原因：LLM 可能在回答里反复标注同一来源（云端模型尤其明显，会生成
    ``[1][2][1][2]...`` 这种），导致前端 citations 标签重复渲染。保留首次出现
    的顺序，过滤不在 valid_ids 里的脏标注。
    """
    valid_ids = {c.citation_id for c in chunks}
    seen: set[int] = set()
    unique_cited: list[int] = []
    for citation_id in cited:
        if citation_id in valid_ids and citation_id not in seen:
            seen.add(citation_id)
            unique_cited.append(citation_id)

    citation_map = {c.citation_id: c for c in chunks}
    return [
        {
            "id": citation_id,
            "file_name": citation_map[citation_id].file_name,
            "page": citation_map[citation_id].page,
            "text": citation_map[citation_id].text,
        }
        for citation_id in unique_cited
    ]


def final_events(
    chunks: list[SourceChunk],
    acc: AnswerAccumulator,
    session_id: str,
    retrieve_ms: int,
) -> list[RagEvent]:
    """流尾事件：citation（有引用时）+ done（含 TTFT / low_confidence 标记）。"""
    events: list[RagEvent] = []
    citations = build_citations(acc.cited, chunks)
    if citations:
        events.append(("citation", {"citations": citations}))
    done_data: dict[str, object] = {
        "session_id": session_id,
        "total_tokens": acc.total_tokens,
        "duration_ms": elapsed_ms(acc.started),
        "retrieve_ms": retrieve_ms,
    }
    # T10.2 TTFT 指标：首 token 相对请求开始的耗时（未生成 token 的路径不输出）
    if acc.first_token_ms is not None:
        done_data["first_token_ms"] = acc.first_token_ms
    if acc.low_confidence:
        done_data["low_confidence"] = True
    events.append(("done", done_data))
    return events
