"""RAG 流式流水线的检索阶段：检索结果整形 + search 事件载荷组装。

由 :mod:`app.api.routes_chat` 拆出（函数行数门禁：单函数 ≤60 行、模块 ≤500 行）：
本模块只放**纯变换**（不发起 IO、不解析全局状态），编排留在路由层，
调用点按阶段再拆为 :mod:`app.api.chat_retrieve`（改写 / 混合检索 / 重排 / 降级）
与 :mod:`app.api.chat_answer`（生成 / 自我纠正 / SSE 帧）——服务层函数名
（rewrite_query / hybrid_search / rerank / stream_generate / stream_with_citations /
validate_answer）在这两个模块内解析，测试须按
``app.api.chat_retrieve.<name>`` / ``app.api.chat_answer.<name>`` 注入替身
（monkeypatch 补丁打在调用点所在模块上，打到 ``routes_chat`` 不会生效）。

生成阶段的累积与 token / citation / done 事件已按阶段拆至
:mod:`app.api.chat_answer_events`（2026-09-18，原文件 310 行超 Python 警告阈值
300）；本模块保留检索侧整形、search 事件与两侧共用的帧类型 / 计时助手。

事件契约见 ``routes_chat`` 模块 docstring（04_API详细规格书 §3.4 + IT-005）。
"""

from __future__ import annotations

import time
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING

from app.services.rag_prompt import SourceChunk
from app.services.rerank_service import RerankCandidate

if TYPE_CHECKING:
    from collections.abc import Sequence

    from app.models import ChatChunkInput, ChatStreamRequest
    from app.services.hybrid_search import FusedHit
    from app.services.rerank_service import RerankResult

#: 检索管线结果：``(rewritten_query, candidates, sources, chunks)``
RetrieveValue = tuple[str, int, list[dict[str, object]], list[SourceChunk]]
#: SSE 事件：``(event_name, data)``
RagEvent = tuple[str, dict[str, object]]


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
