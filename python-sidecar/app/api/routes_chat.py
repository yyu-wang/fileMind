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

import time
import uuid
from typing import TYPE_CHECKING

from fastapi import APIRouter
from fastapi.responses import StreamingResponse

from app import state
from app.api.chat_answer import _answer_events, _sse
from app.api.chat_answer_events import (
    AnswerAccumulator,
    AnswerContext,
    final_events,
)
from app.api.chat_pipeline import (
    RetrieveFailure,
    elapsed_ms,
    no_result_events,
    search_events,
)
from app.api.chat_retrieve import _retrieve_with_fallback
from app.core.logging import getLogger
from app.models import ChatQueryResponse, ChatStreamRequest
from app.services.provider_factory import resolve_cloud_provider, resolve_local_provider

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.api.chat_pipeline import RagEvent
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


def _resolve_chat_provider(request: ChatStreamRequest) -> LLMProvider | None:
    """按请求的推理模式解析生成 Provider。

    推理模式（``inference_mode``）是用户当前生效选择的权威来源：仅显式
    ``cloud`` 才解析云端 Provider，其余（``local``/``hybrid``/未知值）一律
    走本地生成。若不先过滤 ``local``，Rust 启动 Sidecar 时注入的
    ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 环境变量会在撤回云端同意 / 切回本地后
    仍然生效（Sidecar 进程不随模式切换重启，env 冻结），导致本地模型名也被
    :func:`resolve_cloud_provider` 判定为云端并打到云端代理，最终因缺 Key 报 401。

    本地生成按**生效后端**选路（T3）：内置引擎（显式选 builtin，或探测发现
    Ollama 不可用而自动回落）返回内置 Provider；默认（Ollama）返回 ``None``，
    交给调用点既有的 Ollama 路径——零行为变化。

    Args:
        request: 流式请求（含 ``inference_mode`` 与 ``llm_model``）。

    Returns:
        云端 Provider 实例；本地模式返回内置引擎 Provider 或 ``None``
        （``None`` 表示走 Ollama 路径）。
    """
    if request.inference_mode.strip().lower() != "cloud":
        return resolve_local_provider(request.llm_model)
    return resolve_cloud_provider(request.llm_model)


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
