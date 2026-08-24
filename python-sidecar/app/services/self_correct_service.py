"""T5.7 — RAG 自我纠正服务（P-04：幻觉检测 + 引用验证 + 最多重试 2 次）。

来源：08_Prompt工程设计 §5（P-04 self_correct.py）。
生成回答后校验其是否忠实于检索片段：幻觉 / 引用错误 / 数据不准确 / 内容缺失，
发现问题时返回 ``corrected_answer``，由路由侧触发重试（最多 max_retries 次）。

失败兜底（fail-open，对齐 T5.6「已流式发送的回答不因验证故障而中断」）：
  - LLM 超时 → 视为 is_correct=True（跳过纠正，直接输出）
  - JSON 解析失败 → 视为 is_correct=True（reason 标记解析失败）
  - Ollama 连接/HTTP 失败 → 抛 :class:`LLMUnavailableError`，由路由捕获后跳过纠正

复用：``call_ollama_json`` / ``extract_json_text``（P-01/P-02 共用）——生成是
非流式单次调用且要求 JSON（format="json"），与 P-01/P-02 语义一致。
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from pydantic import BaseModel, Field, ValidationError

from app.rules.llm_classify import (
    OLLAMA_TIMEOUT,
    LLMUnavailableError,
    call_ollama_json,
    extract_json_text,
)
from app.services.cloud_provider import CloudUnavailableError

if TYPE_CHECKING:
    from app.services.cloud_provider import LLMProvider, PromptVersion

_SYSTEM_TEMPLATE = """你是一个回答质量审查员。你的任务是检查一个 RAG 回答是否忠实于提供的文档片段，是否存在幻觉或引用错误。

## 检查项
1. 幻觉检测：回答中是否有文档片段未包含的信息？
2. 引用正确性：[N] 引用编号是否指向正确的文档片段？
3. 数据准确性：数字、日期、金额是否与原文一致？
4. 完整性：回答是否遗漏了文档片段中的重要信息？

## 输出格式（严格 JSON）
{{
  "is_correct": true,
  "issues": [],
  "corrected_answer": null
}}

如果发现问题：
{{
  "is_correct": false,
  "issues": ["问题描述1", "问题描述2"],
  "corrected_answer": "修正后的回答"
}}

只输出 JSON，不要输出其他内容。"""

_FEW_SHOT_EXAMPLES = """[示例 1 — 检测到幻觉]
文档片段：[1] 营收为 5.2 亿元
回答：营收为 5.5 亿元 [1]

{"is_correct": false, "issues": ["数据错误：文档片段中营收为5.2亿，回答中为5.5亿"], "corrected_answer": "营收为 5.2 亿元 [1]"}

[示例 2 — 引用错误]
文档片段：[1] 财务报告，[2] 产品文档
回答：根据产品文档，营收为 5.2 亿元 [1]

{"is_correct": false, "issues": ["引用错误：[1]是财务报告不是产品文档"], "corrected_answer": "根据财务报告，营收为 5.2 亿元 [1]"}

[示例 3 — 回答正确]
文档片段：[1] 营收为 5.2 亿元
回答：营收为 5.2 亿元 [1]

{"is_correct": true, "issues": [], "corrected_answer": null}"""

# T8.5 云端变体：利用云端 response_format(json_object) 强约束，指令精简、
# few-shot 3→1；JSON schema 与本地版本完全一致（DoD「同一请求在 local/cloud
# 下输出格式一致」）。
_SYSTEM_TEMPLATE_CLOUD = """你是一个回答质量审查员。检查一个 RAG 回答是否忠实于提供的文档片段，是否存在幻觉或引用错误。

## 检查项
1. 幻觉检测：回答中是否有文档片段未包含的信息？
2. 引用正确性：[N] 引用编号是否指向正确的文档片段？
3. 数据准确性：数字、日期、金额是否与原文一致？
4. 完整性：回答是否遗漏了文档片段中的重要信息？

## 输出格式（严格 JSON）
{{
  "is_correct": true,
  "issues": [],
  "corrected_answer": null
}}

如果发现问题：
{{
  "is_correct": false,
  "issues": ["问题描述1", "问题描述2"],
  "corrected_answer": "修正后的回答"
}}"""

_FEW_SHOT_EXAMPLES_CLOUD = """[示例 1 — 检测到幻觉]
文档片段：[1] 营收为 5.2 亿元
回答：营收为 5.5 亿元 [1]

{"is_correct": false, "issues": ["数据错误：文档片段中营收为5.2亿，回答中为5.5亿"], "corrected_answer": "营收为 5.2 亿元 [1]"}"""

_USER_TEMPLATE = """## 原始问题
{user_query}

## 文档片段（事实依据）
{retrieved_context}

## 待检查的回答
{generated_answer}

## Few-shot 示例

{example}

[待检查]"""

#: 未附 issues 但判定不正确时的默认 retry 原因（对齐 §3.4 示例「答案无引用支撑」）
_DEFAULT_REASON = "答案无引用支撑"


@dataclass(frozen=True)
class SelfCorrectResult:
    """P-04 验证结果。

    - ``is_correct``：回答是否忠实于文档片段。
    - ``issues``：发现的问题描述列表。
    - ``corrected_answer``：修正后的完整回答（is_correct=True 时为 None）。
    - ``reason``：retry 事件用原因（issues 拼接或默认文案）。
    """

    is_correct: bool
    issues: list[str] = field(default_factory=list)
    corrected_answer: str | None = None
    reason: str = ""


class _SelfCorrectResponse(BaseModel):
    """P-04 严格 JSON 输出结构。"""

    is_correct: bool
    issues: list[str] = Field(default_factory=list)
    corrected_answer: str | None = None


def _fallback(reason: str) -> SelfCorrectResult:
    """验证异常兜底：视为正确（fail-open），不触发重试。"""
    return SelfCorrectResult(is_correct=True, reason=reason)


def build_self_correct_prompt(
    query: str,
    context_blocks: str,
    answer: str,
    version: PromptVersion = "local",
) -> tuple[str, str]:
    """按 P-04 模板构建 (system, user) 消息对。

    Args:
        query: 用户查询（改写后结果）。
        context_blocks: 检索片段上下文（与 P-03 同格式，见 format_context_blocks）。
        answer: 待检查的回答（含 [N] 引用标注）。
        version: Prompt 版本（T8.5）。``"cloud"`` 用精简指令 + 单 few-shot；
            ``"local"`` 用详细指令 + 3 few-shot。两者 JSON schema 完全一致。

    Returns:
        (system_prompt, user_prompt) 二元组，直接传入 Ollama chat。
    """
    if version == "cloud":
        system, example = _SYSTEM_TEMPLATE_CLOUD, _FEW_SHOT_EXAMPLES_CLOUD
    else:
        system, example = _SYSTEM_TEMPLATE, _FEW_SHOT_EXAMPLES
    user = _USER_TEMPLATE.format(
        user_query=query,
        retrieved_context=context_blocks,
        generated_answer=answer,
        example=example,
    )
    return system, user


def parse_self_correct_response(raw: str) -> SelfCorrectResult:
    """解析 P-04 JSON 输出；任何异常降级为 is_correct=True（fail-open）。

    - 畸形 JSON / 字段校验失败 → is_correct=True，reason 标记解析失败
    - ``corrected_answer`` 空串 → 归一为 None（无可交付的修正回答）
    - ``reason``：is_correct=False 时取 issues 拼接，空 issues 用默认文案

    Args:
        raw: LLM 返回的原始文本（容忍 markdown 代码块包裹）。

    Returns:
        解析后的验证结果。
    """
    try:
        data = _SelfCorrectResponse.model_validate_json(extract_json_text(raw))
    except ValidationError:
        return _fallback("解析失败")
    except ValueError:
        return _fallback("解析失败")
    corrected = data.corrected_answer or None
    reason = "；".join(data.issues) if data.issues else _DEFAULT_REASON
    return SelfCorrectResult(
        is_correct=data.is_correct,
        issues=data.issues,
        corrected_answer=corrected,
        reason=reason,
    )


async def validate_answer(
    query: str,
    context_blocks: str,
    answer: str,
    provider: LLMProvider | None = None,
) -> SelfCorrectResult:
    """对生成的回答执行 P-04 校验。

    Args:
        query: 用户查询（改写后结果）。
        context_blocks: 检索片段上下文（P-03 同格式）。
        answer: 待检查的回答（含 [N] 引用标注）。
        provider: 推理 Provider（T8.5）。``None`` 走本地 Ollama（默认行为）；
            传入云端 Provider 时用其 ``generate(json_mode=True)`` 并选用对应
            Prompt 版本。

    Returns:
        验证结果；LLM 超时 → is_correct=True（fail-open）。

    Raises:
        LLMUnavailableError: Ollama 连接/HTTP 失败 / 云端不可用（路由侧捕获后
            跳过纠正）。
    """
    version = provider.version if provider is not None else "local"
    system, user = build_self_correct_prompt(query, context_blocks, answer, version=version)
    try:
        if provider is not None:
            try:
                raw = await provider.generate(
                    system, user, json_mode=True, temperature=0.0, max_tokens=256
                )
            except CloudUnavailableError as exc:
                raise LLMUnavailableError(f"云端推理不可用: {exc}") from exc
        else:
            raw = await asyncio.wait_for(call_ollama_json(system, user), timeout=OLLAMA_TIMEOUT)
    except TimeoutError:
        return _fallback("超时")
    return parse_self_correct_response(raw)
