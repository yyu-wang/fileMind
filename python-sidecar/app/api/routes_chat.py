"""RAG 问答路由：/chat/stream SSE 流式输出。

编排（T5.6/T5.7，依赖 T5.3/T5.4/T5.5）：
    查询改写（P-02）→ 混合检索（RRF 融合）→ BGE-reranker 重排序 →
    P-03 流式生成（逐 token + 引用标注）→ P-04 自我纠正（幻觉检测，最多重试）。

FTS5 由 Rust 层执行（Sidecar 永不碰 SQLite），命中含文本经请求体
``fts_chunks`` 传入；本路由只做向量检索 + 融合 + 重排 + 生成 + 自我纠正。

SSE 事件契约（04_API详细规格书 §3.4 + IT-005）：
    search_start → search_result → token* → citation → done / error
    （P-04 检测到问题且重试次数 < max_retries 时：token* → retry → token* → citation → done；
      重试耗尽仍失败：done 带 low_confidence: true；
      检索降级兜底时：search_start 之后、search_result 之前插入 search_warning）
"""

from __future__ import annotations

import json
import time
import uuid
from typing import TYPE_CHECKING

from fastapi import APIRouter
from fastapi.responses import StreamingResponse

from app import state
from app.api.chat_pipeline import (
    AnswerAccumulator,
    AnswerContext,
    RetrieveFailure,
    RetrieveSuccess,
    build_sources,
    elapsed_ms,
    enrich_with_fts_text,
    final_events,
    no_result_events,
    search_events,
    to_rerank_candidates,
    token_events,
)
from app.core.logging import getLogger
from app.models import ChatQueryResponse, ChatStreamRequest, ChatTurn
from app.rules.llm_classify import LLMUnavailableError
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.generation_service import (
    build_rag_prompt,
    format_context_blocks,
    stream_generate,
    stream_with_citations,
)
from app.services.hybrid_search import hybrid_search
from app.services.provider_factory import resolve_cloud_provider
from app.services.query_cache import get_query_cache
from app.services.rerank_service import RerankResult, RerankUnavailableError, rerank
from app.services.rewrite_service import ConversationTurn, rewrite_query
from app.services.self_correct_service import validate_answer

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.api.chat_pipeline import RagEvent, RetrieveValue
    from app.db.lancedb_repo import LanceDBManager
    from app.services.cloud_provider import LLMProvider
    from app.services.hybrid_search import FusedHit
    from app.services.rerank_service import RerankCandidate
    from app.services.self_correct_service import SelfCorrectResult

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


def _resolve_chat_provider(request: ChatStreamRequest) -> LLMProvider | None:
    """按请求的推理模式解析生成 Provider。

    推理模式（``inference_mode``）是用户当前生效选择的权威来源：仅显式
    ``cloud`` 才解析云端 Provider，其余（``local``/``hybrid``/未知值）一律
    回落本地 Ollama（返回 ``None``）。若不先过滤 ``local``，Rust 启动
    Sidecar 时注入的 ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 环境变量会在
    撤回云端同意 / 切回本地后仍然生效（Sidecar 进程不随模式切换重启，
    env 冻结），导致本地模型名也被 :func:`resolve_cloud_provider` 判定为
    云端并打到云端代理，最终因缺 Key 报 401。

    Args:
        request: 流式请求（含 ``inference_mode`` 与 ``llm_model``）。

    Returns:
        云端 Provider 实例；非云端模式返回 ``None``（调用方走 Ollama 路径）。
    """
    if request.inference_mode.strip().lower() != "cloud":
        return None
    return resolve_cloud_provider(request.llm_model)


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
        skip_vector: 跳过向量检索（Embedding 不可用时云端模式降级用）。

    Returns:
        - ``value``：``(rewritten_query, candidates, sources, chunks)`` ——
          ``candidates`` 为重排前候选池大小（search_result 事件字段），
          ``sources`` 为精选 Top-K 的来源列表（含引用编号），
          ``chunks`` 为 P-03 上下文片段（含原文，按 citation_id 升序）。
        - ``degraded``：是否走了纯 FTS5 降级检索（Embedding 不可用）。
        - ``rerank_degraded``：重排是否降级（模型不可用时退回融合排序），供调用方发提示。

    Raises:
        LLMUnavailableError / EmbeddingUnavailableError: 推理（改写 / 向量化）不可用。
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


def _sse(event: str, data: dict[str, object]) -> str:
    """SSE 帧序列化：``event: {name}\\ndata: {json}\\n\\n``。"""
    return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


async def _single_token(text: str) -> AsyncIterator[str]:
    """把 P-04 修正回答作为单块 token 流，复用 stream_with_citations 解析引用。"""
    yield text


async def _answer_token_events(
    ctx: AnswerContext,
    acc: AnswerAccumulator,
) -> AsyncIterator[RagEvent]:
    """P-03 流式生成（首轮）：token 事件转发 + 首 token 指标。"""
    system, user = build_rag_prompt(
        ctx.rewritten_query, ctx.chunks, version=ctx.version, model=ctx.request.llm_model
    )
    valid_ids = {c.citation_id for c in ctx.chunks}
    token_stream = stream_generate(system, user, model=ctx.request.llm_model, provider=ctx.provider)
    async for event in token_events(
        stream_with_citations(token_stream, valid_ids), acc, with_ttft=True
    ):
        yield event


async def _validate_answer(
    ctx: AnswerContext,
    context_blocks: str,
    answer: str,
) -> SelfCorrectResult | None:
    """P-04 答案验证；验证用 LLM 不可用 → ``None``（fail-open，跳过纠正）。"""
    try:
        return await validate_answer(
            ctx.rewritten_query,
            context_blocks,
            answer,
            provider=ctx.provider,
            model=ctx.request.llm_model,
        )
    except LLMUnavailableError:
        return None


async def _correction_events(
    ctx: AnswerContext,
    acc: AnswerAccumulator,
    context_blocks: str,
    result: SelfCorrectResult | None,
) -> AsyncIterator[RagEvent]:
    """P-04 自我纠正：验证失败且重试次数 < max_retries 时重推修正回答。

    Yields:
        ``("retry", ...)`` 与重推修正回答的 ``("token", ...)`` 事件；
        重试耗尽仍不正确 → 置 ``acc.low_confidence``（调用方写入 done）。
    """
    valid_ids = {c.citation_id for c in ctx.chunks}
    used = 0
    while result is not None and not result.is_correct and used < ctx.request.max_retries:
        corrected = result.corrected_answer
        if not corrected:
            # 无可用修正回答 → 保留原答案，仅标记低置信度，不触发 retry
            acc.low_confidence = True
            break
        used += 1
        yield (
            "retry",
            {"reason": result.reason, "attempt": used, "rewritten_query": ctx.rewritten_query},
        )
        # 前端收到 retry 后清空缓冲，重推修正回答的 token 流
        acc.begin_retry()
        async for event in token_events(
            stream_with_citations(_single_token(corrected), valid_ids), acc
        ):
            yield event
        result = await _validate_answer(ctx, context_blocks, "".join(acc.parts))

    if result is not None and not result.is_correct:
        acc.low_confidence = True


async def _answer_events(ctx: AnswerContext, acc: AnswerAccumulator) -> AsyncIterator[RagEvent]:
    """生成 + 自我纠正的事件流（token / retry / error）。

    Yields:
        生成阶段的 ``("token", ...)`` 与纠正阶段事件；生成失败产出 ``("error", ...)``
        并置 ``acc.failed``（调用方据此收尾，不再产出 citation / done）。
    """
    try:
        async for event in _answer_token_events(ctx, acc):
            yield event
    except LLMUnavailableError as exc:
        logger.warning("chat.generate_failed", error=str(exc))
        # SC-m10：生成失败报 LLM_UNAVAILABLE（更准确的语义）
        yield ("error", {"code": "LLM_UNAVAILABLE", "message": str(exc)})
        acc.failed = True
        return

    # P-04 自我纠正：验证失败且重试次数 < max_retries 时重推修正回答（fail-open）
    context_blocks = format_context_blocks(ctx.chunks)
    result = await _validate_answer(ctx, context_blocks, "".join(acc.parts))
    async for event in _correction_events(ctx, acc, context_blocks, result):
        yield event


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
    """检索降级：云端模式 Embedding 不可用 → 纯 FTS5 检索；其余 → 错误事件。"""
    if not isinstance(exc, EmbeddingUnavailableError) or request.inference_mode.lower() != "cloud":
        logger.warning("chat.retrieve_failed", error=str(exc))
        return RetrieveFailure(_error_event(_retrieve_error_code(exc), exc))

    logger.warning("chat.embedding_unavailable_cloud_fallback", error=str(exc))
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


async def _rag_event_stream(
    request: ChatStreamRequest,
    mgr: LanceDBManager,
) -> AsyncIterator[RagEvent]:
    """完整 RAG 流水线事件序列（端点格式化为 SSE 帧）。

    Yields:
        ``(event, data)``：search_start / search_result / token / citation / done / error。
    """
    session_id = request.session_id or uuid.uuid4().hex
    started = time.monotonic()
    # 按请求的推理模式选 Provider：local（含撤回云端同意后的状态）一律本地，
    # 避免 Sidecar 启动期冻结的 FILEMIND_ACTIVE_CLOUD_PROVIDER env 把本地
    # 模型也误判为云端（回归：切回本地仍报 401 的内部错误）。
    provider = _resolve_chat_provider(request)

    retrieve_started = time.monotonic()
    outcome = await _retrieve_with_fallback(request, mgr, provider)
    retrieve_ms = elapsed_ms(retrieve_started)
    if isinstance(outcome, RetrieveFailure):
        yield outcome.event
        return
    logger.info("chat.retrieve.done", ms=retrieve_ms)

    for event in search_events(request, outcome):
        yield event

    rewritten_query, _candidates, _sources, chunks = outcome.value
    if not chunks:
        for event in no_result_events(session_id, started, retrieve_ms, NOT_FOUND_ANSWER):
            yield event
        return

    acc = AnswerAccumulator(started=started)
    ctx = AnswerContext(
        request=request,
        provider=provider,
        rewritten_query=rewritten_query,
        chunks=chunks,
        version=provider.version if provider is not None else "local",
    )
    async for event in _answer_events(ctx, acc):
        yield event
    if acc.failed:
        return
    for event in final_events(chunks, acc, session_id, retrieve_ms):
        yield event


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
        try:
            async for event, data in _rag_event_stream(request, mgr):
                yield _sse(event, data)
        except Exception as exc:
            # 兜底（SC-M2）：LanceDB 表不存在 / table 与 embedding 模型不匹配等
            # 意外异常若不捕获，async generator 中途崩溃 → SSE 连接断开且无
            # error 事件，前端只能看到连接错误。此处产出 error 事件后正常收尾；
            # 异常类型与细节只进服务端日志，不透给前端
            # （SidecarLogger 协议仅承诺 info/warning，与既有失败日志级别一致）
            logger.warning(
                "chat.stream_failed",
                error_type=type(exc).__name__,
                error=str(exc),
            )
            yield _sse("error", {"code": "INTERNAL_ERROR", "message": "生成回答时发生内部错误"})

    return StreamingResponse(
        stream(), media_type="text/event-stream", headers={"Cache-Control": "no-cache"}
    )
