"""RAG 答案流与 SSE 事件（原 routes_chat.py 拆出，超 300 行警告阈值）。

包含：SSE 帧序列化、逐 token 事件、P-04 自我纠正与重试、以及 error 事件。
本模块只负责「把事件按契约吐出来」，编排在 routes_chat._rag_event_stream。
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

from app.api.chat_pipeline import (
    AnswerAccumulator,
    AnswerContext,
    token_events,
)
from app.core.logging import getLogger
from app.rules.llm_classify import LLMUnavailableError
from app.services.generation_service import (
    build_rag_prompt,
    format_context_blocks,
    stream_generate,
    stream_with_citations,
)
from app.services.self_correct_service import validate_answer

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.api.chat_pipeline import RagEvent
    from app.services.self_correct_service import SelfCorrectResult

logger = getLogger("filemind.chat")


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
