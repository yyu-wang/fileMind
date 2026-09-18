"""LLM 兜底分类层：Ollama 调用 + JSON 结果解析 + 单文件分类编排。

来源：08_Prompt工程设计 §2（P-01 分类兜底 Prompt）。
三层分类漏斗第 3 层，规则层（engine）与启发式层（heuristic）均未命中时
调用（预计覆盖约 10% 文件）。P-01 提示词构建已按职责拆至
:mod:`app.rules.llm_classify_prompt`（2026-09-18，原文件 355 行超 Python
警告阈值 300），``CONTENT_SUMMARY_MAX`` 与 ``build_classify_prompt`` 仍由
本模块再导出，外部导入路径不变。

失败兜底（对齐 08-§2）：
- Ollama 连接/HTTP 失败 → 抛 :class:`LLMUnavailableError`（整个第 3 层跳过）
- LLM 超时 → 返回"未分类"（reason 标记 LLM 超时）
- JSON 解析失败 → 返回"未分类"（reason 标记解析失败），不抛给调用方
"""

from __future__ import annotations

import asyncio
import json
import os
from typing import TYPE_CHECKING

import httpx
from ollama import AsyncClient, Message, Options, ResponseError
from pydantic import BaseModel, ValidationError

from app.rules.llm_classify_prompt import (
    CONTENT_SUMMARY_MAX as CONTENT_SUMMARY_MAX,
)
from app.rules.llm_classify_prompt import (
    build_classify_prompt as build_classify_prompt,
)
from app.services.cloud_provider import CloudUnavailableError

if TYPE_CHECKING:
    from app.core.cloud_mask import CloudMasker
    from app.models import ClassifyItem
    from app.services.cloud_provider import LLMProvider

#: 业务判定阈值：LLM 置信度 < 阈值 → 标记"待人工确认"
#: （对齐任务 T4.4「置信度阈值 >0.7」与 API 规格书错误码 LOW_CONFIDENCE）
CONFIDENCE_THRESHOLD = float(os.environ.get("FILEMIND_CONFIDENCE_THRESHOLD", "0.7"))
#: 单文件 LLM 调用超时（秒），本地模型应在 30s 内完成
OLLAMA_TIMEOUT = float(os.environ.get("FILEMIND_OLLAMA_TIMEOUT", "30"))

#: 生成式模型名（env 可覆盖；本机默认已装 qwen3.8-27b）
LLM_MODEL = os.environ.get("FILEMIND_LLM_MODEL", "qwen3.8-27b")
#: Ollama 服务地址（env 可覆盖）
OLLAMA_HOST = os.environ.get("FILEMIND_OLLAMA_URL", "http://127.0.0.1:11434")
#: Ollama 请求保活时间（env 可覆盖）
OLLAMA_KEEP_ALIVE = os.environ.get("FILEMIND_OLLAMA_KEEP_ALIVE", "30m")
#: Ollama 上下文窗口大小（env 可覆盖）
OLLAMA_NUM_CTX = int(os.environ.get("FILEMIND_OLLAMA_NUM_CTX", "8192") or "8192")


class LLMUnavailableError(Exception):
    """Ollama 服务不可用（连接失败 / HTTP 错误）→ 整个第 3 层跳过。"""


# pydantic mypy 插件为 BaseModel 子类生成的 __init__/__eq__ 等成员带显式 Any，
# 与 strict 的 disallow_any_explicit 冲突；错误无法从模型侧消除，仅抑制本行
# （同 app.models 的 disallow_any_explicit=false override 的同类问题，此处更收敛）。
class ClassifyResult(BaseModel):  # type: ignore[explicit-any]
    """P-01 输出的 LLM 分类结果。"""

    category: str
    confidence: float
    reason: str
    is_new_category: bool


def extract_json_text(raw: str) -> str:
    """从 LLM 输出中提取 JSON 文本（P-01/P-02 共用）。

    容忍两种常见包裹：markdown 代码块（`````json ... `````，qwen3 系常见）与
    前后缀文本（按首个 ``{`` 到末个 ``}`` 截取）。无法识别时原样返回，由
    调用方 json 解析兜底。
    """
    text = raw.strip()
    if text.startswith("```"):
        text = "\n".join(text.splitlines()[1:]).rstrip("`").strip()
    start = text.find("{")
    end = text.rfind("}")
    if start != -1 and end != -1 and end > start:
        return text[start : end + 1]
    return text


def parse_classify_response(raw: str) -> ClassifyResult:
    """解析 LLM 返回的分类结果，任何异常都降级为"未分类"，不抛给调用方。

    - JSON/字段校验失败 → ``未分类`` / 0.0 / ``is_new_category=False``
    - 置信度越界（非 [0,1]）→ 降级 0.5（对齐 P-01 输出解析示例）
    - 置信度 < ``CONFIDENCE_THRESHOLD`` → reason 前缀 ``[需人工确认] ``

    Args:
        raw: LLM 返回的原始文本（容忍 markdown 代码块包裹）。

    Returns:
        解析后的分类结果。
    """
    try:
        data = json.loads(extract_json_text(raw))
        if not isinstance(data, dict):
            raise ValueError("LLM 输出不是 JSON 对象")
        result = ClassifyResult(**data)
    except (json.JSONDecodeError, TypeError, ValueError, ValidationError) as exc:
        return ClassifyResult(
            category="未分类",
            confidence=0.0,
            reason=f"LLM 输出解析失败: {exc}",
            is_new_category=False,
        )
    if not 0.0 <= result.confidence <= 1.0:
        result.confidence = 0.5  # 异常值降级
    if result.confidence < CONFIDENCE_THRESHOLD:
        result.reason = f"[需人工确认] {result.reason}"
    return result


async def call_ollama_json(system: str, user: str, *, model: str = LLM_MODEL) -> str:
    """调用 Ollama 生成严格 JSON 结果（P-01 分类 / P-02 查询改写共用）。

    连接/HTTP 失败抛 :class:`LLMUnavailableError`。``format="json"`` 约束输出，
    ``think=False`` 关闭 Qwen3 系列模型的思维链：思考 token 计入
    ``num_predict`` 预算，未关闭时 JSON 会被截断/报 502（非思维模型忽略此参数）。

    Args:
        system: system 提示词。
        user: user 提示词。
        model: 生成模型名（SC-m9：默认 ``LLM_MODEL``，调用方可传 ``request.llm_model``
            确保改写/自纠与生成用同一模型）。
    """
    client = AsyncClient(host=OLLAMA_HOST)
    try:
        resp = await client.chat(
            model=model,
            messages=[
                Message(role="system", content=system),
                Message(role="user", content=user),
            ],
            format="json",
            think=False,
            # keep_alive 是 chat 顶层参数（模型常驻，T10.2），num_ctx 在 Options 里
            keep_alive=OLLAMA_KEEP_ALIVE,
            options=Options(num_predict=256, temperature=0.0, num_ctx=OLLAMA_NUM_CTX),
        )
    except (httpx.HTTPError, ConnectionError, ResponseError) as exc:
        raise LLMUnavailableError(f"Ollama 调用失败: {exc}") from exc
    # 注意：ollama SDK 的 ChatResponse/Message 运行时并非 Mapping ABC（无
    # isinstance(resp, Mapping) 判定），但均继承 SubscriptableBaseModel.get()，
    # 故用鸭子类型的 .get() 取值，仅对 content 做类型校验。
    message = resp.get("message")
    if message is None:
        return ""
    content = message.get("content")
    if not isinstance(content, str):
        return ""
    return content


async def classify_file_with_llm(
    item: ClassifyItem,
    categories: list[str],
    masker: CloudMasker | None = None,
    provider: LLMProvider | None = None,
) -> ClassifyResult:
    """对单个文件执行 LLM 兜底分类（P-01 调用 + JSON 解析）。

    失败兜底（对齐 08-§2）：
      - Ollama 连接/HTTP 失败 / 云端不可用 → 抛 :class:`LLMUnavailableError`
        （调用方跳过第 3 层）
      - LLM 超时 → 返回``未分类``（reason 标记 LLM 超时），不抛异常
      - JSON 解析失败 → 返回``未分类``，不抛异常

    Args:
        item: 待分类文件信息。
        categories: 预定义分类列表。
        masker: 云端脱敏器（批处理共享实例，保证编号连续）。
        provider: 推理 Provider（T8.5）。``None`` 走本地 Ollama（默认行为）；
            传入云端 Provider 时用其 ``generate(json_mode=True)`` 并选用对应
            Prompt 版本（``provider.version``）。

    Returns:
        解析后的分类结果。
    """
    version = provider.version if provider is not None else "local"
    system, user = build_classify_prompt(item, categories, masker, version=version)
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
        return ClassifyResult(
            category="未分类",
            confidence=0.0,
            reason="LLM 超时，已跳过该文件",
            is_new_category=False,
        )
    return parse_classify_response(raw)
