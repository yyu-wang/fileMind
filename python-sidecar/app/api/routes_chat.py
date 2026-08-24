"""RAG 问答路由：/chat/stream SSE 流式输出。

编排（T5.6/T5.7，依赖 T5.3/T5.4/T5.5）：
    查询改写（P-02）→ 混合检索（RRF 融合）→ BGE-reranker 重排序 →
    P-03 流式生成（逐 token + 引用标注）→ P-04 自我纠正（幻觉检测，最多重试）。

FTS5 由 Rust 层执行（Sidecar 永不碰 SQLite），命中含文本经请求体
``fts_chunks`` 传入；本路由只做向量检索 + 融合 + 重排 + 生成 + 自我纠正。

SSE 事件契约（04_API详细规格书 §3.4 + IT-005）：
    search_start → search_result → token* → citation → done / error
    （P-04 检测到问题且重试次数 < max_retries 时：token* → retry → token* → citation → done；
      重试耗尽仍失败：done 带 low_confidence: true）
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
    format_context_blocks,
    stream_generate,
    stream_with_citations,
)
from app.services.hybrid_search import hybrid_search
from app.services.provider_factory import resolve_cloud_provider
from app.services.query_cache import get_query_cache
from app.services.rerank_service import RerankCandidate, RerankUnavailableError, rerank
from app.services.rewrite_service import ConversationTurn, rewrite_query
from app.services.self_correct_service import validate_answer

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.db.lancedb_repo import LanceDBManager
    from app.services.cloud_provider import LLMProvider

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


def _cache_key(request: ChatStreamRequest) -> tuple[object, ...]:
    """检索缓存 key：覆盖影响检索结果的输入。

    FTS 命中（``fts_chunks``）由同一 query 在 Rust 侧确定性产生，不入 key；
    ``llm_model`` 只影响生成阶段，不影响检索结果。
    """
    history = tuple((turn.user, turn.assistant) for turn in request.history)
    return (
        request.query,
        history,
        request.table_name,
        request.embedding_model,
        request.top_k,
        request.rerank_top_k,
    )


async def _retrieve(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
    provider: LLMProvider | None,
) -> tuple[str, int, list[dict[str, object]], list[SourceChunk]]:
    """改写 → 混合检索 → 重排序，返回 (rewritten_query, candidates, sources, chunks)。

    Args:
        request: 流式请求（含 FTS 命中与对话历史）。
        mgr: LanceDB 管理器（向量检索）。
        provider: 推理 Provider（T8.5）；``None`` 走本地 Ollama（默认）。

    Returns:
        - ``rewritten_query``：改写后的查询（向量检索 + 生成上下文用）。
        - ``candidates``：重排前候选池大小（search_result 事件字段）。
        - ``sources``：精选 Top-K 的来源列表（含引用编号）。
        - ``chunks``：P-03 上下文片段（含原文，按 citation_id 升序）。

    Raises:
        LLMUnavailableError / EmbeddingUnavailableError / RerankUnavailableError:
            推理（改写 / 向量化 / 重排）不可用。
    """
    # T10.2 查询缓存：相同请求命中时整条检索管线跳过（改写/向量化/重排归零）。
    cache = get_query_cache()
    cache_key = _cache_key(request)
    cached = await cache.get(cache_key)
    if cached is not None:
        logger.info("chat.retrieve.cache_hit")
        return cached

    t_stage = time.monotonic()
    rewritten = await rewrite_query(request.query, _to_turns(request.history), provider=provider)
    rewritten_query = rewritten.rewritten_query
    logger.info("chat.retrieve.rewrite", ms=_elapsed_ms(t_stage))

    t_stage = time.monotonic()
    fused = await hybrid_search(
        rewritten_query,
        [c.chunk_id for c in request.fts_chunks],
        mgr,
        request.table_name,
        model=request.embedding_model,
        top_k=request.top_k,
    )
    logger.info("chat.retrieve.hybrid_search", ms=_elapsed_ms(t_stage))

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
    t_stage = time.monotonic()
    reranked = await rerank(rewritten_query, candidates, top_k=request.rerank_top_k)
    logger.info("chat.retrieve.rerank", ms=_elapsed_ms(t_stage))

    hit_by_id = {h.chunk_id: h for h in enriched}
    sources: list[dict[str, object]] = []
    chunks: list[SourceChunk] = []
    for idx, reranked_hit in enumerate(reranked, start=1):
        hit = hit_by_id.get(reranked_hit.chunk_id)
        text = hit.chunk_text if hit else ""
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
                text=text,
            )
        )
    retrieved = (rewritten_query, len(candidates), sources, chunks)
    await cache.set(cache_key, retrieved)
    return retrieved


def _sse(event: str, data: dict[str, object]) -> str:
    """SSE 帧序列化：``event: {name}\\ndata: {json}\\n\\n``。"""
    return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


def _elapsed_ms(started: float) -> int:
    """自 started（monotonic）以来的毫秒数。"""
    return int((time.monotonic() - started) * 1000)


async def _single_token(text: str) -> AsyncIterator[str]:
    """把 P-04 修正回答作为单块 token 流，复用 stream_with_citations 解析引用。"""
    yield text


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
    provider = resolve_cloud_provider(request.llm_model)
    version = provider.version if provider is not None else "local"

    retrieve_started = time.monotonic()
    try:
        rewritten_query, candidates, sources, chunks = await _retrieve(request, mgr, provider)
    except (LLMUnavailableError, EmbeddingUnavailableError, RerankUnavailableError) as exc:
        logger.warning("chat.retrieve_failed", error=str(exc))
        yield ("error", {"code": "OLLAMA_UNAVAILABLE", "message": str(exc)})
        return
    retrieve_ms = _elapsed_ms(retrieve_started)
    logger.info("chat.retrieve.done", ms=retrieve_ms)

    yield ("search_start", {"query_original": request.query, "query_rewritten": rewritten_query})
    yield (
        "search_result",
        {"candidates": candidates, "after_rerank": len(sources), "sources": sources},
    )

    if not chunks:
        yield ("token", {"content": NOT_FOUND_ANSWER})
        yield (
            "done",
            {
                "session_id": session_id,
                "total_tokens": 1,
                "duration_ms": _elapsed_ms(started),
                "retrieve_ms": retrieve_ms,
            },
        )
        return

    system, user = build_rag_prompt(
        rewritten_query, chunks, version=version, model=request.llm_model
    )
    valid_ids = {c.citation_id for c in chunks}
    total_tokens = 0
    cited: list[int] = []
    answer_parts: list[str] = []
    first_token_ms: int | None = None
    try:
        token_stream = stream_generate(system, user, model=request.llm_model, provider=provider)
        async for event in stream_with_citations(token_stream, valid_ids):
            if event[0] == "text":
                if first_token_ms is None:
                    first_token_ms = _elapsed_ms(started)
                    logger.info("chat.first_token", ttft_ms=first_token_ms)
                yield ("token", {"content": event[1]})
                answer_parts.append(event[1])
                total_tokens += 1
            else:
                cited.append(event[1])
                answer_parts.append(f"[{event[1]}]")
    except LLMUnavailableError as exc:
        logger.warning("chat.generate_failed", error=str(exc))
        yield ("error", {"code": "OLLAMA_UNAVAILABLE", "message": str(exc)})
        return

    # P-04 自我纠正：验证失败且重试次数 < max_retries 时重推修正回答（fail-open）
    context_blocks = format_context_blocks(chunks)
    try:
        result = await validate_answer(
            rewritten_query, context_blocks, "".join(answer_parts), provider=provider
        )
    except LLMUnavailableError:
        result = None  # 验证不可用 → 跳过纠正，直接输出已生成回答

    low_confidence = False
    used = 0
    while result is not None and not result.is_correct and used < request.max_retries:
        corrected = result.corrected_answer
        if not corrected:
            # 无可用修正回答 → 保留原答案，仅标记低置信度，不触发 retry
            low_confidence = True
            break
        used += 1
        yield (
            "retry",
            {"reason": result.reason, "attempt": used, "rewritten_query": rewritten_query},
        )
        # 前端收到 retry 后清空缓冲，重推修正回答的 token 流
        new_parts: list[str] = []
        new_cited: list[int] = []
        async for event in stream_with_citations(_single_token(corrected), valid_ids):
            if event[0] == "text":
                yield ("token", {"content": event[1]})
                new_parts.append(event[1])
                total_tokens += 1
            else:
                new_cited.append(event[1])
                new_parts.append(f"[{event[1]}]")
        cited = new_cited
        answer_text = "".join(new_parts)
        try:
            result = await validate_answer(
                rewritten_query, context_blocks, answer_text, provider=provider
            )
        except LLMUnavailableError:
            result = None

    if result is not None and not result.is_correct:
        low_confidence = True

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
    done_data: dict[str, object] = {
        "session_id": session_id,
        "total_tokens": total_tokens,
        "duration_ms": _elapsed_ms(started),
        "retrieve_ms": retrieve_ms,
    }
    # T10.2 TTFT 指标：首 token 相对请求开始的耗时（未生成 token 的路径不输出）
    if first_token_ms is not None:
        done_data["first_token_ms"] = first_token_ms
    if low_confidence:
        done_data["low_confidence"] = True
    yield ("done", done_data)


@router.post("/stream")
async def chat_stream(request: ChatStreamRequest) -> StreamingResponse:
    """RAG 问答 SSE 流式输出（改写 → 检索 → 重排 → 生成 → 自我纠正）。

    Args:
        request: 流式请求（含 FTS 命中与对话历史）。

    Returns:
        ``text/event-stream``：事件序列 search_start → search_result → token*
        → citation → done；P-04 触发重试时插入 retry 事件；本地推理失败发 error。
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
