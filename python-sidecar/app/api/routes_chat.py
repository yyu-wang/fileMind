"""RAG 问答路由：/chat/stream SSE 流式输出。

编排（T5.6，依赖 T5.3/T5.4/T5.5）：
    查询改写（P-02）→ 混合检索（RRF 融合）→ BGE-reranker 重排序 →
    P-03 流式生成（逐 token + 引用标注）。

FTS5 由 Rust 层执行（Sidecar 永不碰 SQLite），命中含文本经请求体
``fts_chunks`` 传入；本路由只做向量检索 + 融合 + 重排 + 生成。

SSE 事件契约（04_API详细规格书 §3.4 + IT-005）：
    search_start → search_result → token* → citation → done / error
"""

from __future__ import annotations

import dataclasses
import json
import time
import uuid
from pathlib import Path
from typing import TYPE_CHECKING

from fastapi import APIRouter
from fastapi.responses import StreamingResponse

from app import state
from app.core.logging import getLogger
from app.models import ChatQueryResponse, ChatStreamRequest, ChatTurn
from app.rules.llm_classify import LLMUnavailableError
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.generation_service import (
    SourceChunk,
    build_rag_prompt,
    stream_generate,
    stream_with_citations,
)
from app.services.hybrid_search import hybrid_search
from app.services.rerank_service import RerankCandidate, RerankUnavailableError, rerank
from app.services.rewrite_service import ConversationTurn, rewrite_query

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.db.lancedb_repo import LanceDBManager

router = APIRouter(prefix="/chat", tags=["RAG 问答"])

logger = getLogger("filemind.chat")

#: 检索无结果时的兜底回答（P-03 回答规则 3）
NOT_FOUND_ANSWER = "根据现有文档，未找到相关信息"


@router.post("/query", response_model=ChatQueryResponse)
async def query() -> ChatQueryResponse:
    """RAG 问答非流式查询（占位，T6.x 按流式聚合实现）。"""
    return ChatQueryResponse(answer="", citations=[], tokens=0)


def _to_turns(history: list[ChatTurn]) -> list[ConversationTurn]:
    """ChatTurn → ConversationTurn（P-02 查询改写输入）。"""
    return [ConversationTurn(user=turn.user, assistant=turn.assistant) for turn in history]


async def _retrieve(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
) -> tuple[str, int, list[dict[str, object]], list[SourceChunk]]:
    """改写 → 混合检索 → 重排序，返回 (rewritten_query, candidates, sources, chunks)。

    Args:
        request: 流式请求（含 FTS 命中与对话历史）。
        mgr: LanceDB 管理器（向量检索）。

    Returns:
        - ``rewritten_query``：改写后的查询（向量检索 + 生成上下文用）。
        - ``candidates``：重排前候选池大小（search_result 事件字段）。
        - ``sources``：精选 Top-K 的来源列表（含引用编号）。
        - ``chunks``：P-03 上下文片段（含原文，按 citation_id 升序）。

    Raises:
        LLMUnavailableError / EmbeddingUnavailableError / RerankUnavailableError:
            本地推理（改写 / 向量化 / 重排）不可用。
    """
    rewritten = await rewrite_query(request.query, _to_turns(request.history))
    rewritten_query = rewritten.rewritten_query

    fused = await hybrid_search(
        rewritten_query,
        [c.chunk_id for c in request.fts_chunks],
        mgr,
        request.table_name,
        model=request.embedding_model,
        top_k=request.top_k,
    )

    # FTS-only 命中的原文在 Rust 侧（SQLite），用请求 fts_chunks 补全
    text_by_id = {c.chunk_id: c.text for c in request.fts_chunks}
    enriched = [
        dataclasses.replace(hit, chunk_text=hit.chunk_text or text_by_id.get(hit.chunk_id, ""))
        for hit in fused
    ]

    candidates = [
        RerankCandidate(hit.chunk_id, hit.chunk_text, hit.rrf_score, hit.file_path, hit.page)
        for hit in enriched
        if hit.chunk_text
    ]
    reranked = await rerank(rewritten_query, candidates, top_k=request.rerank_top_k)

    hit_by_id = {h.chunk_id: h for h in enriched}
    sources: list[dict[str, object]] = []
    chunks: list[SourceChunk] = []
    for idx, result in enumerate(reranked, start=1):
        hit = hit_by_id.get(result.chunk_id)
        text = hit.chunk_text if hit else ""
        file_name = Path(result.file_path).name if result.file_path else ""
        sources.append(
            {"id": idx, "file_name": file_name, "page": result.page, "score": result.score}
        )
        chunks.append(
            SourceChunk(citation_id=idx, file_name=file_name, page=result.page, text=text)
        )
    return rewritten_query, len(candidates), sources, chunks


def _sse(event: str, data: dict[str, object]) -> str:
    """SSE 帧序列化：``event: {name}\\ndata: {json}\\n\\n``。"""
    return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


def _elapsed_ms(started: float) -> int:
    """自 started（monotonic）以来的毫秒数。"""
    return int((time.monotonic() - started) * 1000)


async def _rag_event_stream(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
) -> AsyncIterator[tuple[str, dict[str, object]]]:
    """完整 RAG 流水线事件序列（端点格式化为 SSE 帧）。

    Yields:
        ``(event, data)``：search_start / search_result / token / citation / done / error。
    """
    session_id = request.session_id or uuid.uuid4().hex
    started = time.monotonic()

    try:
        rewritten_query, candidates, sources, chunks = await _retrieve(request, mgr)
    except (LLMUnavailableError, EmbeddingUnavailableError, RerankUnavailableError) as exc:
        logger.warning("chat.retrieve_failed", error=str(exc))
        yield ("error", {"code": "OLLAMA_UNAVAILABLE", "message": str(exc)})
        return

    yield ("search_start", {"query_original": request.query, "query_rewritten": rewritten_query})
    yield (
        "search_result",
        {"candidates": candidates, "after_rerank": len(sources), "sources": sources},
    )

    if not chunks:
        yield ("token", {"content": NOT_FOUND_ANSWER})
        yield (
            "done",
            {"session_id": session_id, "total_tokens": 1, "duration_ms": _elapsed_ms(started)},
        )
        return

    system, user = build_rag_prompt(rewritten_query, chunks)
    valid_ids = {c.citation_id for c in chunks}
    total_tokens = 0
    cited: list[int] = []
    try:
        token_stream = stream_generate(system, user, model=request.llm_model)
        async for event in stream_with_citations(token_stream, valid_ids):
            if event[0] == "text":
                yield ("token", {"content": event[1]})
                total_tokens += 1
            else:
                cited.append(event[1])
    except LLMUnavailableError as exc:
        logger.warning("chat.generate_failed", error=str(exc))
        yield ("error", {"code": "OLLAMA_UNAVAILABLE", "message": str(exc)})
        return

    citation_map = {c.citation_id: c for c in chunks}
    citations = [
        {
            "id": citation_id,
            "file_name": citation_map[citation_id].file_name,
            "page": citation_map[citation_id].page,
            "text": citation_map[citation_id].text,
        }
        for citation_id in cited
    ]
    if citations:
        yield ("citation", {"citations": citations})
    yield (
        "done",
        {
            "session_id": session_id,
            "total_tokens": total_tokens,
            "duration_ms": _elapsed_ms(started),
        },
    )


@router.post("/stream")
async def chat_stream(request: ChatStreamRequest) -> StreamingResponse:
    """RAG 问答 SSE 流式输出（查询改写 → 混合检索 → 重排序 → P-03 生成）。

    Args:
        request: 流式请求（含 FTS 命中与对话历史）。

    Returns:
        ``text/event-stream``：事件序列 search_start → search_result → token*
        → citation → done（本地推理失败发 error）。
    """
    mgr = state.get_lancedb()

    async def stream() -> AsyncIterator[str]:
        if mgr is None:
            logger.warning("chat.lancedb_unavailable")
            yield _sse("error", {"code": "INTERNAL_ERROR", "message": "向量库未初始化"})
            return
        async for event, data in _rag_event_stream(request, mgr):
            yield _sse(event, data)

    return StreamingResponse(
        stream(), media_type="text/event-stream", headers={"Cache-Control": "no-cache"}
    )
