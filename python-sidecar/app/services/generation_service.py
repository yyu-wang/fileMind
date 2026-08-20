"""T5.6 — RAG 生成服务（P-03：检索上下文 + 流式生成 + 引用解析）。

来源：08_Prompt工程设计 §4（P-03 rag_prompt.py）+ §4 流式引用解析。
把重排序后的 Top-5 检索片段组装为 P-03 上下文，流式调用 Ollama 生成回答，
并在 token 流中实时解析 ``[N]`` 引用标注。

引用解析（08-§4 算法）：``[N]`` 标记前的文本作为普通 token 片段，标记本身
作为 citation 片段；不在 ``valid_ids`` 中的标记（幻觉引用）直接丢弃。

与 P-01/P-02 的差异：生成是**流式**且非 JSON 输出（P-03 明确「不要输出 JSON」），
故不复用 ``call_ollama_json``，但复用其常量与异常（``OLLAMA_HOST``、
``LLM_MODEL``、``LLMUnavailableError``）。

错误策略：连接/HTTP 失败抛 :class:`LLMUnavailableError`；不用整体超时——
流式响应必须跑完，中途掐断会丢后半回答（与 P-01 单次调用的超时语义不同）。
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

import httpx
from ollama import AsyncClient, Message, Options, ResponseError

from app.rules.llm_classify import LLM_MODEL, OLLAMA_HOST, LLMUnavailableError

if TYPE_CHECKING:
    from collections.abc import AsyncIterator
    from typing import Literal

    from ollama import ChatResponse

#: P-03 单个检索片段内容截断长度（对齐输入变量 source_N_content「限 500 字符」）
CONTENT_MAX = 500

#: 引用标注模式：``[N]``
CITATION_PATTERN = re.compile(r"\[(\d+)\]")

_SYSTEM_TEMPLATE = """你是一个知识库问答助手。基于检索到的文档片段回答用户问题。

## 回答规则
1. 仅基于提供的文档片段回答，不使用外部知识
2. 每段事实陈述必须标注引用来源，格式：[引用编号]
3. 如果文档片段中没有相关信息，明确回答"根据现有文档，未找到相关信息"
4. 如果多个文档片段信息冲突，指出冲突并列出各方来源
5. 回答使用中文，语言简洁专业
6. 如果问题涉及具体数据（金额、日期、数字），必须原文引用，不得改写

## 输出格式
直接输出回答文本，在事实陈述后标注 [引用编号]。
不要输出 JSON，不要输出思考过程。"""

_FEW_SHOT_EXAMPLE = """[示例]
问题：2024年Q3的营收增长率是多少？

文档片段：
[1] 来源：2024Q3财务报告.pdf（第 3 页）
内容：2024年第三季度营收为 5.2 亿元，去年同期为 4.5 亿元。

[2] 来源：2024Q3财务报告.pdf（第 5 页）
内容：营收同比增长 15.6%，环比增长 3.2%。

根据财务报告，2024年Q3营收为 5.2 亿元，去年同期为 4.5 亿元 [1]。营收同比增长 15.6% [2]。"""

_USER_TEMPLATE = """## 用户问题
{user_query}

## 检索到的文档片段
（每段标注引用编号，含文件名和页码）

{context_blocks}

## Few-shot 示例

{example}

[待回答]
问题：{user_query}

文档片段：
{context_blocks}"""


@dataclass(frozen=True)
class SourceChunk:
    """P-03 单条检索片段（对应一个引用编号）。"""

    citation_id: int  # 1..N，回答中的 [N] 引用编号
    file_name: str
    page: int
    text: str


def _context_block(chunk: SourceChunk) -> str:
    """单片段格式：``[N] 来源：{file_name}（第 {page} 页）\\n内容：{text}``。"""
    return (
        f"[{chunk.citation_id}] 来源：{chunk.file_name}"
        f"（第 {chunk.page} 页）\n内容：{chunk.text[:CONTENT_MAX]}"
    )


def format_context_blocks(chunks: list[SourceChunk]) -> str:
    """把检索片段格式化为上下文块（P-03 生成与 P-04 验证共用同一事实依据）。

    Args:
        chunks: 重排序后的检索片段（按 citation_id 升序）。

    Returns:
        多片段 ``\\n\\n`` 连接文本。
    """
    return "\n\n".join(_context_block(c) for c in chunks)


def build_rag_prompt(query: str, chunks: list[SourceChunk]) -> tuple[str, str]:
    """按 P-03 模板构建 (system, user) 消息对。

    Args:
        query: 用户查询（改写后结果，P-03 输入变量 user_query）。
        chunks: 重排序后的检索片段（Top-K，默认 5），按 citation_id 升序。

    Returns:
        (system_prompt, user_prompt) 二元组，直接传入流式聊天调用。
    """
    user = _USER_TEMPLATE.format(
        user_query=query,
        context_blocks=format_context_blocks(chunks),
        example=_FEW_SHOT_EXAMPLE,
    )
    return _SYSTEM_TEMPLATE, user


async def stream_generate(
    system: str,
    user: str,
    model: str = LLM_MODEL,
) -> AsyncIterator[str]:
    """流式调用 Ollama 生成回答，逐块 yield 内容 delta。

    Args:
        system: system 提示词。
        user: user 提示词。
        model: 生成模型名（默认 ``LLM_MODEL``）。

    Yields:
        非空内容 delta（Ollama 逐 token 推送）。

    Raises:
        LLMUnavailableError: Ollama 连接/HTTP 失败（首个 delta 前抛出）。
    """
    client = AsyncClient(host=OLLAMA_HOST)
    try:
        # chat() 是 async def，stream=True 时 await 后得到流式迭代器
        stream = cast(
            "AsyncIterator[ChatResponse]",
            await client.chat(
                model=model,
                messages=[
                    Message(role="system", content=system),
                    Message(role="user", content=user),
                ],
                stream=True,
                # 关闭 qwen3 思维链：思考 token 混入回答流会破坏引用标注（同 P-01）
                think=False,
                options=Options(temperature=0.2),
            ),
        )
        async for chunk in stream:
            message = chunk.message
            if message is None:
                continue
            content = message.content
            if content:
                yield content
    except (httpx.HTTPError, ResponseError) as exc:
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
