"""RAG 检索阶段与降级兜底（原 routes_chat.py 拆出，超 300 行警告阈值）。

包含：检索缓存 key、查询改写（P-02）、混合检索（RRF）、BGE-reranker 重排序，
以及 Embedding/LLM 不可用时的纯 FTS5 降级路径（T4）。
"""

from __future__ import annotations

import time
from typing import TYPE_CHECKING

from app.api.chat_pipeline import (
    RetrieveFailure,
    RetrieveSuccess,
    build_sources,
    elapsed_ms,
    enrich_with_fts_text,
    to_rerank_candidates,
)
from app.core.logging import getLogger
from app.rules.llm_classify import LLMUnavailableError
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.hybrid_search import hybrid_search
from app.services.query_cache import get_query_cache
from app.services.rerank_service import RerankResult, RerankUnavailableError, rerank
from app.services.rewrite_service import ConversationTurn, rewrite_query

if TYPE_CHECKING:
    from app.api.chat_pipeline import RagEvent, RetrieveValue
    from app.db.lancedb_repo import LanceDBManager
    from app.models import ChatStreamRequest, ChatTurn
    from app.services.cloud_provider import LLMProvider
    from app.services.hybrid_search import FusedHit
    from app.services.rerank_service import RerankCandidate

logger = getLogger("filemind.chat")


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


async def _rewrite_stage(request: ChatStreamRequest, provider: LLMProvider | None) -> str:
    """P-02 查询改写（记录阶段耗时日志）。"""
    started = time.monotonic()
    rewritten = await rewrite_query(
        request.query, _to_turns(request.history), provider=provider, model=request.llm_model
    )
    logger.info("chat.retrieve.rewrite", ms=elapsed_ms(started))
    return rewritten.rewritten_query


async def _search_stage(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
    rewritten_query: str,
    *,
    skip_vector: bool,
) -> list[FusedHit]:
    """混合检索：向量 + FTS 命中 RRF 融合（记录阶段耗时日志）。"""
    started = time.monotonic()
    fused = await hybrid_search(
        rewritten_query,
        [c.chunk_id for c in request.fts_chunks],
        mgr,
        request.table_name,
        model=request.embedding_model,
        top_k=request.top_k,
        skip_vector=skip_vector,
    )
    logger.info("chat.retrieve.hybrid_search", ms=elapsed_ms(started))
    return fused


async def _rerank_stage(
    rewritten_query: str,
    candidates: list[RerankCandidate],
    top_k: int,
) -> tuple[list[RerankResult], bool]:
    """BGE-reranker 精排；模型不可用 → 退回融合序 Top-K（降级，不中断问答）。

    Returns:
        ``(重排结果, 是否降级)``：降级时按 RRF 融合分取 Top-K，score 为融合分。
    """
    started = time.monotonic()
    try:
        reranked = await rerank(rewritten_query, candidates, top_k=top_k)
    except RerankUnavailableError as exc:
        # 降级兜底：重排模型缺失（未下载 / HF 缓存被清）或推理失败时，退回 RRF 融合序
        # 取 Top-K —— 问答质量降级而非整条请求失败（原行为直接中断并报 RERANK_UNAVAILABLE）。
        logger.warning("chat.retrieve.rerank_degraded", ms=elapsed_ms(started), error=str(exc))
        return [
            RerankResult(c.chunk_id, c.score, c.file_path, c.page) for c in candidates[:top_k]
        ], True
    logger.info("chat.retrieve.rerank", ms=elapsed_ms(started))
    return reranked, False


async def _retrieve(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
    provider: LLMProvider | None,
    skip_vector: bool = False,
) -> RetrieveSuccess:
    """改写 → 混合检索 → 重排序，返回检索结果与降级标记。

    Args:
        request: 流式请求（含 FTS 命中与对话历史）。
        mgr: LanceDB 管理器（向量检索）。
        provider: 推理 Provider（T8.5）；``None`` 走本地 Ollama（默认）。
        skip_vector: 跳过向量检索（Embedding 不可用时的降级用）。

    Returns:
        - ``value``：``(rewritten_query, candidates, sources, chunks)`` ——
          ``candidates`` 为重排前候选池大小（search_result 事件字段），
          ``sources`` 为精选 Top-K 的来源列表（含引用编号），
          ``chunks`` 为 P-03 上下文片段（含原文，按 citation_id 升序）。
        - ``degraded``：是否走了纯 FTS5 降级检索（Embedding 不可用）。
        - ``rerank_degraded``：重排是否降级（模型不可用时退回融合排序），供调用方发提示。

    Raises:
        EmbeddingUnavailableError: 查询向量化不可用（模型未下载等）；
            非 ``skip_vector`` 路径下由调用方转降级检索。
        LLMUnavailableError: 仅 ``skip_vector=False`` 时可能由改写/检索管线抛出
            （改写内部已降级，故当前实际不触发；保留声明供后续扩展）。
    """
    # T10.2 查询缓存：相同请求命中时整条检索管线跳过（改写/向量化/重排归零）。
    # 降级模式（skip_vector=True）不缓存，以便 embedding 恢复后能重试全向量检索。
    cache = get_query_cache()
    cache_key = _cache_key(request)
    if not skip_vector:
        cached = await cache.get(cache_key)
        if cached is not None:
            logger.info("chat.retrieve.cache_hit")
            return RetrieveSuccess(cached)

    rewritten_query = await _rewrite_stage(request, provider)
    fused = await _search_stage(request, mgr, rewritten_query, skip_vector=skip_vector)
    enriched = enrich_with_fts_text(fused, request.fts_chunks)
    candidates = to_rerank_candidates(enriched)
    reranked, rerank_degraded = await _rerank_stage(
        rewritten_query, candidates, request.rerank_top_k
    )
    sources, chunks = build_sources(reranked, enriched)

    retrieved: RetrieveValue = (rewritten_query, len(candidates), sources, chunks)
    # 降级结果（skip_vector / rerank_degraded）不缓存：模型恢复后同一问句立刻重试完整管线。
    if not skip_vector and not rerank_degraded:
        await cache.set(cache_key, retrieved)
    return RetrieveSuccess(retrieved, degraded=skip_vector, rerank_degraded=rerank_degraded)


def _error_event(code: str, exc: Exception) -> RagEvent:
    """error 事件（错误码 + 异常文案）。"""
    return ("error", {"code": code, "message": str(exc)})


def _retrieve_error_code(exc: Exception) -> str:
    """检索错误码：SC-m10 区分 LLM/Embedding 不可用（不统一报 OLLAMA_UNAVAILABLE）。

    Rerank 不可用已不在此列：``_retrieve`` 内部降级为融合排序，不会抛到本层。
    """
    if isinstance(exc, EmbeddingUnavailableError):
        return "EMBEDDING_UNAVAILABLE"
    return "LLM_UNAVAILABLE"


async def _fallback_retrieve(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
    provider: LLMProvider | None,
    exc: Exception,
) -> RetrieveSuccess | RetrieveFailure:
    """检索降级：Embedding 不可用（任何推理模式）→ 纯 FTS5 检索；其余 → 错误事件。

    不限云端模式：Embedding 模型未下载 / 加载失败是**与推理模式无关**的本地状态，
    local 模式下同样应以质量降级换取可用性（对齐 rerank 的降级取舍），而不是
    整条问答中断——未下载模型的机器上用户至少还能拿到关键词命中的结果，并由
    ``search_warning(EMBEDDING_DEGRADED)`` 得知质量下降与修复入口。
    """
    if not isinstance(exc, EmbeddingUnavailableError):
        logger.warning("chat.retrieve_failed", error=str(exc))
        return RetrieveFailure(_error_event(_retrieve_error_code(exc), exc))

    logger.warning(
        "chat.embedding_unavailable_fallback",
        error=str(exc),
        inference_mode=request.inference_mode,
    )
    try:
        return await _retrieve(request, mgr, provider, skip_vector=True)
    except LLMUnavailableError as fallback_exc:
        logger.warning("chat.retrieve_fallback_failed", error=str(fallback_exc))
        return RetrieveFailure(_error_event(_retrieve_error_code(fallback_exc), fallback_exc))


async def _retrieve_with_fallback(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
    provider: LLMProvider | None,
) -> RetrieveSuccess | RetrieveFailure:
    """检索（含 Embedding 不可用时的降级）；失败返回 error 事件（不抛异常）。"""
    try:
        return await _retrieve(request, mgr, provider)
    except (LLMUnavailableError, EmbeddingUnavailableError) as exc:
        return await _fallback_retrieve(request, mgr, provider, exc)
