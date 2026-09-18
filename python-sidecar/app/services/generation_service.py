"""T5.6 — RAG 生成服务（P-03：流式生成 + 引用解析）。

来源：08_Prompt工程设计 §4（P-03 rag_prompt.py）+ §4 流式引用解析。
把重排序后的 Top-5 检索片段组装为 P-03 上下文，流式调用 Ollama 生成回答，
并在 token 流中实时解析 ``[N]`` 引用标注。提示词与上下文组装已按职责拆至
:mod:`app.services.rag_prompt`（2026-09-18，原文件 349 行超 Python 警告阈值
300），本模块只保留流式调用与引用解析。

引用解析（08-§4 算法）：``[N]`` 标记前的文本作为普通 token 片段，标记本身
作为 citation 片段；不在 ``valid_ids`` 中的标记（幻觉引用）直接丢弃。

与 P-01/P-02 的差异：生成是**流式**且非 JSON 输出（P-03 明确「不要输出 JSON」），
故不复用 ``call_ollama_json``，但复用其常量与异常（``OLLAMA_HOST``、
``LLM_MODEL``、``LLMUnavailableError``）。

错误策略：连接/HTTP 失败抛 :class:`LLMUnavailableError`；不用整体超时——
流式响应必须跑完，中途掐断会丢后半回答（与 P-01 单次调用的超时语义不同）。
"""

from __future__ import annotations

import asyncio
import os
import re
from typing import TYPE_CHECKING

import httpx
from ollama import AsyncClient, Message, Options, ResponseError

from app.rules.llm_classify import (
    LLM_MODEL,
    OLLAMA_HOST,
    OLLAMA_KEEP_ALIVE,
    OLLAMA_NUM_CTX,
    LLMUnavailableError,
)

if TYPE_CHECKING:
    from collections.abc import AsyncIterator
    from typing import Literal

    from app.services.cloud_provider import LLMProvider

#: SC-m8：本地流式生成总量超时（秒）；Ollama 卡死时防后台流无限挂起
LOCAL_STREAM_TIMEOUT = int(os.environ.get("FILEMIND_LOCAL_STREAM_TIMEOUT", "180") or "180")

#: 引用标注模式：``[N]`` 或常见变体（``[引用N]`` / ``[引N]`` / ``[cite N]`` 等）。
#:
#: 说明：设计约定格式为 ``[N]``（few-shot 示例和本地 qwen 严格遵循）。
#: 但云端模型（deepseek/gpt-4o 等）按提示词中「引用编号」字面理解，
#: 会产出 ``[引用1]`` / ``[引用 2]`` 等形式；正则兼容常见格式，统一以
#: 数字捕获组作为引用编号。
CITATION_PATTERN = re.compile(
    r"\[(?:引用|引|cite|citation|来源)?\s*(\d+)\s*\]",
    re.IGNORECASE,
)

#: 模块级惰性单例 AsyncClient（httpx 连接池复用，避免每次生成重建连接）
_client: AsyncClient | None = None
_client_lock = asyncio.Lock()


async def _get_client() -> AsyncClient:
    """返回模块级 AsyncClient 单例（并发安全，双重检查 + asyncio.Lock）。"""
    global _client
    if _client is None:
        async with _client_lock:
            if _client is None:
                _client = AsyncClient(host=OLLAMA_HOST)
    return _client


def reset_clients() -> None:
    """重置客户端单例（仅测试用）。"""
    global _client
    _client = None


async def stream_generate(
    system: str,
    user: str,
    model: str = LLM_MODEL,
    provider: LLMProvider | None = None,
) -> AsyncIterator[str]:
    """流式调用 LLM 生成回答，逐块 yield 内容 delta。

    Args:
        system: system 提示词。
        user: user 提示词。
        model: 生成模型名（默认 ``LLM_MODEL``；``provider`` 传入时仅作回退值）。
        provider: 推理 Provider（T8.5）。非 ``None`` 时委托其
            ``generate_stream``（云端流式）；``None`` 走本地 Ollama（默认行为）。

    Yields:
        非空内容 delta（Ollama 逐 token 推送）。

    Raises:
        LLMUnavailableError: Ollama 连接/HTTP 失败 / 云端不可用
            （首个 delta 前抛出）。
    """
    if provider is not None:
        # 用独立变量名，避免与下方本地路径的 ``chunk``（ChatResponse）类型冲突
        async for delta in provider.generate_stream(system, user, temperature=0.2):
            yield delta
        return
    client = await _get_client()
    try:
        # chat() 是 async def，stream=True 时 await 后得到流式迭代器；
        # ollama 类型 stub 已标 return 为 AsyncIterator[ChatResponse]，无需 cast
        stream = await client.chat(
            model=model,
            messages=[
                Message(role="system", content=system),
                Message(role="user", content=user),
            ],
            stream=True,
            # 关闭 qwen3 思维链：思考 token 混入回答流会破坏引用标注（同 P-01）
            think=False,
            # keep_alive 是 chat 顶层参数（模型常驻，T10.2），num_ctx 在 Options 里
            keep_alive=OLLAMA_KEEP_ALIVE,
            options=Options(temperature=0.2, num_ctx=OLLAMA_NUM_CTX),
        )
        # SC-m8：本地流式加 180s 总量超时——Ollama GPU 死锁等卡死时防后台流无限挂起
        async with asyncio.timeout(LOCAL_STREAM_TIMEOUT):
            async for chunk in stream:
                message = chunk.message
                if message is None:
                    continue
                content = message.content
                if content:
                    yield content
    except TimeoutError as exc:
        raise LLMUnavailableError(f"Ollama 流式生成超时（>{LOCAL_STREAM_TIMEOUT}s）") from exc
    except (httpx.HTTPError, ConnectionError, ResponseError) as exc:
        raise LLMUnavailableError(f"Ollama 流式生成失败: {exc}") from exc


async def stream_with_citations(
    token_stream: AsyncIterator[str],
    valid_ids: set[int],
) -> AsyncIterator[tuple[Literal["text"], str] | tuple[Literal["citation"], int]]:
    """在流式输出中实时解析 ``[N]`` 引用标注。

    Args:
        token_stream: 上游 token delta 流。
        valid_ids: 合法引用编号集合（来自 search_result 的 sources）。

    Yields:
        ``("text", str)`` 普通文本片段（引用标记前的正文）或
        ``("citation", int)`` 引用编号（标记在 ``valid_ids`` 内才产出，
        否则该标记被丢弃——幻觉引用不上报）。
    """
    buffer = ""
    async for token in token_stream:
        buffer += token
        while match := CITATION_PATTERN.search(buffer):
            if match.start() > 0:
                yield ("text", buffer[: match.start()])
            citation_id = int(match.group(1))
            if citation_id in valid_ids:
                yield ("citation", citation_id)
            buffer = buffer[match.end() :]
    if buffer:
        yield ("text", buffer)
