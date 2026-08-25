"""T5.5 — 查询改写服务（P-02：代词消解 + 关键词扩展 + 历史上下文）。

来源：08_Prompt工程设计 §3（P-02 query_rewrite.py）。
RAG 流水线检索前置步骤：把多轮对话中的用户查询改写为独立、完整的检索查询，
并提取 2-5 个检索关键词供 FTS5 补充。改写后查询用于向量检索，关键词用于
FTS5，两路经 RRF 融合（T5.6 编排）。

失败兜底（对齐 P-01 的退化语义，改写失败不得阻塞检索）：
- 无对话历史 → 不调 LLM，原样返回（P-02 改写策略第 1 步短路径）
- LLM 超时 → 原样返回（``need_rewrite=False``，reason 标记超时）
- JSON 解析失败 / 空 rewritten_query → 原样返回（reason 标记解析失败）
- Ollama 连接/HTTP 失败 → 抛 :class:`LLMUnavailableError`（调用方决定降级）

LLM 调用复用 ``llm_classify.call_ollama_json``：P-01/P-02 同为「system + user
严格 JSON 输出」，无需复制网络/错误处理层。
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import TYPE_CHECKING

from pydantic import BaseModel, Field, ValidationError

from app.rules.llm_classify import (
    LLM_MODEL,
    OLLAMA_TIMEOUT,
    LLMUnavailableError,
    call_ollama_json,
    extract_json_text,
)
from app.services.cloud_provider import CloudUnavailableError

if TYPE_CHECKING:
    from collections.abc import Sequence

    from app.services.cloud_provider import LLMProvider, PromptVersion

#: 参与改写的最近对话轮数（P-02 输入变量 conversation_history「最近 3 轮」）
HISTORY_TURNS = 3

_SYSTEM_TEMPLATE = """你是一个查询改写助手。你的任务是将用户在多轮对话中的查询改写为独立、完整的检索查询。

## 改写规则
1. 将代词（这个、那个、它）替换为上文中的具体指代对象
2. 补充必要的上下文信息，使查询可以独立理解
3. 保持用户原始意图，不添加用户未表达的内容
4. 如果用户查询已经完整，直接返回原文
5. 改写后的查询用于向量检索，应包含关键词和语义信息

## 输出格式（严格 JSON）
{{
  "rewritten_query": "改写后的查询",
  "need_rewrite": true,
  "expanded_keywords": ["关键词1", "关键词2"]
}}"""

_FEW_SHOT_EXAMPLES = """[示例 1 — 代词消解]
对话历史：
用户：2024年Q3的营收是多少？
助手：根据财务报告，2024年Q3营收为 5.2 亿元...
用户：那利润呢？

{"rewritten_query": "2024年Q3的利润是多少", "need_rewrite": true, "expanded_keywords": ["2024", "Q3", "利润", "财务报告"]}

[示例 2 — 查询扩展]
对话历史：
用户：怎么整理照片文件？
助手：FileMind 可以按日期、地点自动分类照片...
用户：视频也行吗？

{"rewritten_query": "FileMind 是否支持视频文件的自动分类整理", "need_rewrite": true, "expanded_keywords": ["FileMind", "视频", "自动分类", "文件整理"]}

[示例 3 — 无需改写]
对话历史：（无）
用户：什么是增量索引？

{"rewritten_query": "什么是增量索引", "need_rewrite": false, "expanded_keywords": ["增量索引"]}"""

# T8.5 云端变体：利用云端 response_format(json_object) 强约束，指令精简、
# few-shot 3→1；JSON schema 与本地版本完全一致（DoD「同一请求在 local/cloud
# 下输出格式一致」）。
_SYSTEM_TEMPLATE_CLOUD = """你是一个查询改写助手。将多轮对话中的查询改写为独立、完整的检索查询。

## 改写规则
1. 将代词（这个、那个、它）替换为上文中的具体指代对象
2. 补充必要的上下文信息，使查询可以独立理解
3. 保持用户原始意图，不添加用户未表达的内容
4. 如果用户查询已经完整，直接返回原文

## 输出格式（严格 JSON）
{{
  "rewritten_query": "改写后的查询",
  "need_rewrite": true,
  "expanded_keywords": ["关键词1", "关键词2"]
}}"""

_FEW_SHOT_EXAMPLES_CLOUD = """[示例 1 — 代词消解]
对话历史：
用户：2024年Q3的营收是多少？
助手：根据财务报告，2024年Q3营收为 5.2 亿元...
用户：那利润呢？

{"rewritten_query": "2024年Q3的利润是多少", "need_rewrite": true, "expanded_keywords": ["2024", "Q3", "利润", "财务报告"]}"""

_USER_TEMPLATE = """## 对话历史（最近 3 轮）
{conversation_history}

## Few-shot 示例

{examples}

[待改写]
对话历史：
{conversation_history}

用户查询：{user_query}"""


@dataclass(frozen=True)
class ConversationTurn:
    """一轮对话（用户提问 + 助手回答），供 P-02 组装对话历史。"""

    user: str
    assistant: str


class RewriteResult(BaseModel):
    """P-02 改写结果。

    ``reason`` 为服务侧注入的降级标记（超时 / 解析失败），LLM 不输出该字段。
    """

    rewritten_query: str
    need_rewrite: bool
    expanded_keywords: list[str] = Field(default_factory=list)
    reason: str = ""


def format_history(history: Sequence[ConversationTurn]) -> str:
    """取最近 3 轮对话并格式化为 ``"用户：xxx\\n助手：xxx"``（多轮间空行分隔）。

    Args:
        history: 完整对话历史（按时间正序）。

    Returns:
        最近 ``HISTORY_TURNS`` 轮的格式化文本；空历史返回空串。
    """
    recent = history[-HISTORY_TURNS:]
    blocks = [f"用户：{turn.user}\n助手：{turn.assistant}" for turn in recent]
    return "\n\n".join(blocks)


def build_rewrite_prompt(
    query: str,
    history: Sequence[ConversationTurn],
    version: PromptVersion = "local",
) -> tuple[str, str]:
    """按 P-02 模板构建 (system, user) 消息对。

    Args:
        query: 当前用户输入的原始查询。
        history: 对话历史（内部取最近 3 轮格式化）。
        version: Prompt 版本（T8.5）。``"cloud"`` 用精简指令 + 单 few-shot；
            ``"local"`` 用详细指令 + 3 few-shot。两者 JSON schema 完全一致。

    Returns:
        (system_prompt, user_prompt) 二元组，直接传入 ``call_ollama_json``。
    """
    if version == "cloud":
        system, examples = _SYSTEM_TEMPLATE_CLOUD, _FEW_SHOT_EXAMPLES_CLOUD
    else:
        system, examples = _SYSTEM_TEMPLATE, _FEW_SHOT_EXAMPLES
    history_text = format_history(history)
    user = _USER_TEMPLATE.format(
        conversation_history=history_text,
        examples=examples,
        user_query=query,
    )
    return system, user


def _fallback(query: str, reason: str) -> RewriteResult:
    """解析失败 / 超时的降级结果：原查询原样返回，不阻塞检索。"""
    return RewriteResult(rewritten_query=query, need_rewrite=False, reason=reason)


def parse_rewrite_response(raw: str, original_query: str) -> RewriteResult:
    """解析 LLM 改写结果，任何异常降级为原查询（不抛给调用方）。

    - JSON/字段校验失败、非对象、``rewritten_query`` 为空串 → 原查询
    - ``expanded_keywords`` 含非字符串元素等 Pydantic 校验失败 → 同理兜底

    Args:
        raw: LLM 返回的原始文本（容忍 markdown 代码块/前后缀包裹）。
        original_query: 用户原始查询（降级时原样返回）。

    Returns:
        改写结果；解析失败时 ``rewritten_query == original_query``。
    """
    try:
        data = extract_json_text(raw)
        result = RewriteResult.model_validate_json(data)
    except (ValidationError, ValueError) as exc:
        return _fallback(original_query, f"LLM 输出解析失败: {exc}")
    if not result.rewritten_query:
        return _fallback(original_query, "LLM 输出解析失败: 改写结果为空")
    return result


async def rewrite_query(
    query: str,
    history: Sequence[ConversationTurn],
    provider: LLMProvider | None = None,
    *,
    model: str = LLM_MODEL,
) -> RewriteResult:
    """对用户查询执行 P-02 改写（代词消解 + 关键词扩展）。

    Args:
        query: 用户原始查询。
        history: 对话历史；为空时不调 LLM，原样返回（改写策略第 1 步）。
        provider: 推理 Provider（T8.5）。``None`` 走本地 Ollama（默认行为，
            ``LLM_MODEL`` / ``call_ollama_json``）；传入云端 Provider 时用其
            ``generate(json_mode=True)`` 并选用对应 Prompt 版本。

    Returns:
        改写结果；无历史 / 超时 / 解析失败时 ``rewritten_query`` 为原查询。

    Raises:
        LLMUnavailableError: Ollama 连接/HTTP 失败 / 云端不可用
            （调用方决定降级）。
    """
    if not history:
        return RewriteResult(rewritten_query=query, need_rewrite=False)
    version = provider.version if provider is not None else "local"
    system, user = build_rewrite_prompt(query, history, version=version)
    try:
        if provider is not None:
            try:
                raw = await provider.generate(
                    system, user, json_mode=True, temperature=0.0, max_tokens=256
                )
            except CloudUnavailableError as exc:
                raise LLMUnavailableError(f"云端推理不可用: {exc}") from exc
        else:
            # SC-m9：本地路径传 model 确保改写与生成用同一模型
            raw = await asyncio.wait_for(
                call_ollama_json(system, user, model=model), timeout=OLLAMA_TIMEOUT
            )
    except TimeoutError:
        return _fallback(query, f"LLM 超时（>{OLLAMA_TIMEOUT}s）")
    return parse_rewrite_response(raw, query)
