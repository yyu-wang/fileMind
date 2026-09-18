"""RAG 流式流水线的纯逻辑：检索结果整形 + SSE 事件载荷组装。

由 :mod:`app.api.routes_chat` 拆出（函数行数门禁：单函数 ≤60 行、模块 ≤500 行）：
本模块只放**纯变换**（不发起 IO、不解析全局状态），编排留在路由层，
调用点按阶段再拆为 :mod:`app.api.chat_retrieve`（改写 / 混合检索 / 重排 / 降级）
与 :mod:`app.api.chat_answer`（生成 / 自我纠正 / SSE 帧）——服务层函数名
（rewrite_query / hybrid_search / rerank / stream_generate / stream_with_citations /
validate_answer）在这两个模块内解析，测试须按
``app.api.chat_retrieve.<name>`` / ``app.api.chat_answer.<name>`` 注入替身
（monkeypatch 补丁打在调用点所在模块上，打到 ``routes_chat`` 不会生效）。

事件契约见 ``routes_chat`` 模块 docstring（04_API详细规格书 §3.4 + IT-005）。
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import TYPE_CHECKING

from app.core.logging import getLogger
from app.services.rag_prompt import SourceChunk
from app.services.rerank_service import RerankCandidate

if TYPE_CHECKING:
    from collections.abc import AsyncIterator, Sequence

    from app.models import ChatChunkInput, ChatStreamRequest
    from app.services.cloud_provider import LLMProvider, PromptVersion
    from app.services.hybrid_search import FusedHit
    from app.services.rerank_service import RerankResult

logger = getLogger("filemind.chat")

#: 检索管线结果：``(rewritten_query, candidates, sources, chunks)``
RetrieveValue = tuple[str, int, list[dict[str, object]], list[SourceChunk]]
#: SSE 事件：``(event_name, data)``
RagEvent = tuple[str, dict[str, object]]
#: ``stream_with_citations`` 的产出：``("text", 正文)`` 或 ``("citation", 编号)``
CitationEvent = tuple[str, str | int]


def elapsed_ms(started: float) -> int:
    """自 started（monotonic）以来的毫秒数。"""
    return int((time.monotonic() - started) * 1000)


@dataclass(frozen=True)
class RetrieveSuccess:
    """检索成功。

    Attributes:
        value: 检索管线结果（改写查询 / 候选数 / sources / 上下文片段）。
        degraded: 是否走了 Embedding 降级（纯 FTS5）路径。
        rerank_degraded: 是否走了重排降级（模型不可用 → 退回 RRF 融合序 Top-K）路径。
    """

    value: RetrieveValue
    degraded: bool = False
    rerank_degraded: bool = False


@dataclass(frozen=True)
class RetrieveFailure:
    """检索失败：``event`` 为直接收尾的 error 事件。"""

    event: RagEvent


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


def _chunk_text_of(hit: FusedHit, text_by_file_id: dict[str, str]) -> str:
    """命中原文：向量路自带文本则直用，否则按文件 ID 前缀回填 FTS 文本。

    LanceDB chunk_id 格式为 ``{file_id}-{seq}``，FTS 返回的是纯 file_id；
    回填时需去掉向量 chunk 的 seq 后缀，用文件 ID 前缀匹配 FTS 文本。
    """
    if hit.chunk_text:
        return hit.chunk_text
    file_id = hit.chunk_id.rsplit("-", 1)[0] if "-" in hit.chunk_id else hit.chunk_id
    return text_by_file_id.get(file_id, "")


def enrich_with_fts_text(
    fused: list[FusedHit],
    fts_chunks: Sequence[ChatChunkInput],
) -> list[FusedHit]:
    """FTS-only 命中的原文在 Rust 侧（SQLite），用请求 fts_chunks 补全。"""
    text_by_file_id = {c.chunk_id: c.text for c in fts_chunks}
    return [replace(hit, chunk_text=_chunk_text_of(hit, text_by_file_id)) for hit in fused]


def to_rerank_candidates(enriched: list[FusedHit]) -> list[RerankCandidate]:
    """重排候选：无原文的命中不可重排，直接过滤。"""
    return [
        RerankCandidate(hit.chunk_id, hit.chunk_text, hit.rrf_score, hit.file_path, hit.page)
        for hit in enriched
        if hit.chunk_text
    ]


def build_sources(
    reranked: list[RerankResult],
    enriched: list[FusedHit],
) -> tuple[list[dict[str, object]], list[SourceChunk]]:
    """把重排结果映射为 search_result.sources 与 P-03 上下文片段（编号从 1 起）。"""
    hit_by_id = {h.chunk_id: h for h in enriched}
    sources: list[dict[str, object]] = []
    chunks: list[SourceChunk] = []
    for idx, reranked_hit in enumerate(reranked, start=1):
        hit = hit_by_id.get(reranked_hit.chunk_id)
        file_name = Path(reranked_hit.file_path).name if reranked_hit.file_path else ""
        sources.append(
            {
                "id": idx,
                "file_name": file_name,
                "page": reranked_hit.page,
                "score": reranked_hit.score,
            }
        )
        chunks.append(
            SourceChunk(
                citation_id=idx,
                file_name=file_name,
                page=reranked_hit.page,
                text=hit.chunk_text if hit else "",
            )
        )
    return sources, chunks


def search_events(request: ChatStreamRequest, outcome: RetrieveSuccess) -> list[RagEvent]:
    """检索阶段事件：search_start / search_warning（降级时）/ search_result。

    降级提示必须发在 search_start 之后：前端的 search_start 分支会清空 error
    字段，发在其前会被立刻覆盖，用户看不到降级说明（静默降级）。
    """
    rewritten_query, candidates, sources, _chunks = outcome.value
    events: list[RagEvent] = [
        ("search_start", {"query_original": request.query, "query_rewritten": rewritten_query})
    ]
    if outcome.degraded:
        events.append(
            (
                "search_warning",
                {
                    "code": "EMBEDDING_DEGRADED",
                    "message": (
                        "Embedding 模型不可用，已降级为纯关键词检索"
                        "（可在「设置 → Embedding 模型」下载后恢复完整检索）"
                    ),
                },
            )
        )
    if outcome.rerank_degraded:
        events.append(
            (
                "search_warning",
                {
                    "code": "RERANK_DEGRADED",
                    "message": "重排模型不可用，已降级为融合排序（结果相关性可能下降）",
                },
            )
        )
    events.append(
        (
            "search_result",
            {"candidates": candidates, "after_rerank": len(sources), "sources": sources},
        )
    )
    return events


def no_result_events(
    session_id: str,
    started: float,
    retrieve_ms: int,
    answer: str,
) -> list[RagEvent]:
    """无检索结果的兜底事件：单条 token + done（total_tokens=1，无 citation）。"""
    return [
        ("token", {"content": answer}),
        (
            "done",
            {
                "session_id": session_id,
                "total_tokens": 1,
                "duration_ms": elapsed_ms(started),
                "retrieve_ms": retrieve_ms,
            },
        ),
    ]


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
