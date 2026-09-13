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

import asyncio
import os
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

import httpx
from ollama import AsyncClient, Message, Options, ResponseError

from app.core.cloud_mask import CloudMasker, content_max, is_cloud_masking_active
from app.rules.llm_classify import (
    LLM_MODEL,
    OLLAMA_HOST,
    OLLAMA_KEEP_ALIVE,
    OLLAMA_NUM_CTX,
    LLMUnavailableError,
)
from app.services.provider_factory import truncate_context

if TYPE_CHECKING:
    from collections.abc import AsyncIterator
    from typing import Literal

    from app.services.cloud_provider import LLMProvider, PromptVersion

#: P-03 单个检索片段内容截断长度（对齐输入变量 source_N_content「限 500 字符」）
CONTENT_MAX = 500

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

_SYSTEM_TEMPLATE = """你是一个知识库问答助手。基于检索到的文档片段回答用户问题。

## 回答规则
1. 仅基于提供的文档片段回答，不使用外部知识
2. 每段事实陈述后必须标注引用来源，格式为 `[数字]`（例：`[1]`、`[2]`，其中数字对应下方文档片段的编号）
3. 如果文档片段中没有相关信息，明确回答"根据现有文档，未找到相关信息"
4. 如果多个文档片段信息冲突，指出冲突并列出各方来源编号
5. 回答使用中文，语言简洁专业
6. 如果问题涉及具体数据（金额、日期、数字），必须原文引用，不得改写

## 输出格式
直接输出回答文本，在事实陈述后直接写 `[N]`（N 为来源编号，不要写成 [引用N]、[来源N] 等变体）。
不要输出 JSON，不要输出思考过程。"""

_FEW_SHOT_EXAMPLE = """[示例]
问题：2024年Q3的营收增长率是多少？

文档片段：
[1] 来源：2024Q3财务报告.pdf（第 3 页）
内容：2024年第三季度营收为 5.2 亿元，去年同期为 4.5 亿元。

[2] 来源：2024Q3财务报告.pdf（第 5 页）
内容：营收同比增长 15.6%，环比增长 3.2%。

根据财务报告，2024年Q3营收为 5.2 亿元，去年同期为 4.5 亿元 [1]。营收同比增长 15.6% [2]。"""

# T8.5 云端变体：虽然云端指令遵循强，但之前出现过"引用编号"被字面
# 化为 [引用1][引用2] 的情况，因此补一个最小示例 + 字面格式模板（对齐本地）；
# few-shot 仅保留单条事实，避免上下文膨胀。
_SYSTEM_TEMPLATE_CLOUD = """你是一个知识库问答助手。基于检索到的文档片段回答用户问题。

## 回答规则
1. 仅基于提供的文档片段回答，不使用外部知识
2. 每段事实陈述后必须标注引用来源，格式为 `[数字]`（例：`[1]`、`[2]`，其中数字对应下方文档片段的编号）
3. 如果文档片段中没有相关信息，明确回答"根据现有文档，未找到相关信息"
4. 如果多个文档片段信息冲突，指出冲突并列出各方来源编号
5. 回答使用中文，语言简洁专业
6. 如果问题涉及具体数据（金额、日期、数字），必须原文引用，不得改写

## 输出格式
直接输出回答文本，在事实陈述后直接写 `[N]`（N 为来源编号，**不要**写成 `[引用N]`、`[来源N]`、`[第N条]` 等任何变体）。
不要输出 JSON，不要输出思考过程。"""

# 云端最小 few-shot（用于输出格式强约束，比长 few-shot 更省上下文且避免
# 云端把 [引用编号] 再字面化）：只演示 [N] 字面格式。
_FEW_SHOT_EXAMPLE_CLOUD = """[示例]
问题：2024年Q3的营收增长率是多少？

文档片段：
[1] 来源：2024Q3财务报告.pdf（第 3 页）
内容：营收同比增长 15.6%，环比增长 3.2%。

2024年Q3营收同比增长 15.6% [1]。"""

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

# T8.5 云端变体：补最小 few-shot 段（仅 [N] 字面格式示例，省上下文但强约束输出格式）。
# 之前省掉后 deepseek 按规则"引用编号"字面生成 [引用1][引用2]，导致解析器识别失败。
_USER_TEMPLATE_CLOUD = """## 用户问题
{user_query}

## 检索到的文档片段
（每段标注来源编号，例：[1]、[2]）

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


def _context_block(chunk: SourceChunk, masker: CloudMasker | None = None) -> str:
    """单片段格式：``[N] 来源：{file_name}（第 {page} 页）\\n内容：{text}``。

    云端脱敏激活时来源替换为编号（``file_001``），不暴露真实文件名。
    """
    label = chunk.file_name
    if masker is not None:
        # 以 file_name 为 key：同文件多片段共享同一编号，供 LLM 引用还原
        masked = masker.mask_file(chunk.file_name, chunk.file_name, None, chunk.text)
        label = masked.mask_name
    return (
        f"[{chunk.citation_id}] 来源：{label}"
        f"（第 {chunk.page} 页）\n内容：{chunk.text[:CONTENT_MAX]}"
    )


def format_context_blocks(
    chunks: list[SourceChunk],
    masker: CloudMasker | None = None,
) -> str:
    """把检索片段格式化为上下文块（P-03 生成与 P-04 验证共用同一事实依据）。

    Args:
        chunks: 重排序后的检索片段（按 citation_id 升序）。
        masker: 云端脱敏器；``None`` 且云端脱敏激活时自动创建（单次调用兜底）。

    Returns:
        多片段 ``\\n\\n`` 连接文本。
    """
    if masker is None and is_cloud_masking_active():
        masker = CloudMasker(content_max())
    return "\n\n".join(_context_block(c, masker) for c in chunks)


def build_rag_prompt(
    query: str,
    chunks: list[SourceChunk],
    masker: CloudMasker | None = None,
    version: PromptVersion = "local",
    model: str | None = None,
) -> tuple[str, str]:
    """按 P-03 模板构建 (system, user) 消息对。

    Args:
        query: 用户查询（改写后结果，P-03 输入变量 user_query）。
        chunks: 重排序后的检索片段（Top-K，默认 5），按 citation_id 升序。
        masker: 云端脱敏器；``None`` 且云端脱敏激活时自动创建。
        version: Prompt 版本（T8.5）。``"cloud"`` 用精简指令（省去 few-shot，
            云端上下文窗口大）；``"local"`` 用完整指令 + few-shot。两者输出
            格式要求完全一致。
        model: 生成模型名（T8.5 Token 长度适配）。非 ``None`` 时对组装好的
            检索上下文按模型上下文窗口截断（``truncate_context``），预留生成空间。

    Returns:
        (system_prompt, user_prompt) 二元组，直接传入流式聊天调用。
    """
    if masker is None and is_cloud_masking_active():
        masker = CloudMasker(content_max())
    context_blocks = format_context_blocks(chunks, masker)
    if model is not None:
        context_blocks = truncate_context(context_blocks, model)
    if version == "cloud":
        system = _SYSTEM_TEMPLATE_CLOUD
        user = _USER_TEMPLATE_CLOUD.format(
            user_query=query,
            context_blocks=context_blocks,
            example=_FEW_SHOT_EXAMPLE_CLOUD,
        )
    else:
        system = _SYSTEM_TEMPLATE
        user = _USER_TEMPLATE.format(
            user_query=query,
            context_blocks=context_blocks,
            example=_FEW_SHOT_EXAMPLE,
        )
    return system, user


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
