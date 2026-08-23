"""LLM 兜底分类层：P-01 提示词构建 + Ollama 调用 + JSON 结果解析。

来源：08_Prompt工程设计 §2（P-01 分类兜底 Prompt）。
三层分类漏斗第 3 层，规则层（engine）与启发式层（heuristic）均未命中时
调用（预计覆盖约 10% 文件）。

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

from app.core.cloud_mask import CloudMasker, content_max, is_cloud_masking_active
from app.services.cloud_provider import CloudUnavailableError

if TYPE_CHECKING:
    from app.models import ClassifyItem
    from app.services.cloud_provider import LLMProvider, PromptVersion

#: 业务判定阈值：LLM 置信度 < 阈值 → 标记"待人工确认"
#: （对齐任务 T4.4「置信度阈值 >0.7」与 API 规格书错误码 LOW_CONFIDENCE）
CONFIDENCE_THRESHOLD = float(os.environ.get("FILEMIND_CONFIDENCE_THRESHOLD", "0.7"))
#: 单文件 LLM 调用超时（秒），本地模型应在 30s 内完成
OLLAMA_TIMEOUT = float(os.environ.get("FILEMIND_OLLAMA_TIMEOUT", "30"))
#: 内容摘要截断长度（对齐 P-01 输入变量 content_summary 前 500 字符）
CONTENT_SUMMARY_MAX = 500

#: 生成式模型名（env 可覆盖；本机默认已装 qwen3.8-27b）
LLM_MODEL = os.environ.get("FILEMIND_LLM_MODEL", "qwen3.8-27b")
#: Ollama 服务地址（env 可覆盖）
OLLAMA_HOST = os.environ.get("FILEMIND_OLLAMA_URL", "http://127.0.0.1:11434")


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


_SYSTEM_TEMPLATE = """你是一个专业的文件分类助手。你的任务是根据文件的元数据和内容摘要，为文件推荐一个最合适的分类。

## 分类规则
1. 从预定义分类列表中选择，如果都不合适可以提出新分类
2. 必须给出置信度（0.0-1.0），低于 0.6 表示不确定
3. 分类名称用中文，简洁明了（2-6 个字）
4. 一个文件只给一个主分类

## 预定义分类列表
{predefined_categories}

## 输出格式（严格 JSON）
{{
  "category": "分类名称",
  "confidence": 0.85,
  "reason": "简短理由（不超过30字）",
  "is_new_category": false
}}"""

_FEW_SHOT_EXAMPLES = """[示例 1]
文件名：2024Q3财务报告.xlsx
文件类型：xlsx
内容摘要：季度营收、利润、现金流数据...
{"category": "财务报表", "confidence": 0.95, "reason": "Excel财务数据文件，含营收利润", "is_new_category": false}

[示例 2]
文件名：IMG_3287.HEIC
文件类型：HEIC
内容摘要：（无文本内容）
{"category": "照片", "confidence": 0.7, "reason": "HEIC格式图片，推测为照片", "is_new_category": false}

[示例 3]
文件名：未命名_最终版_修改3.docx
文件类型：docx
内容摘要：关于产品迭代方向的讨论，包含用户调研结果和竞品分析...
{"category": "产品文档", "confidence": 0.82, "reason": "含产品迭代和用户调研内容", "is_new_category": false}"""

# T8.5 云端变体：利用云端 response_format(json_object) 强约束，指令精简、
# few-shot 3→1；JSON schema 与本地版本完全一致（DoD「同一请求在 local/cloud
# 下输出格式一致」）。
_SYSTEM_TEMPLATE_CLOUD = """你是一个专业的文件分类助手。根据文件的元数据和内容摘要，为文件推荐一个最合适的分类。

## 分类规则
1. 从预定义分类列表中选择，都不合适可提出新分类
2. 分类名称用中文，简洁明了（2-6 个字）

## 预定义分类列表
{predefined_categories}

## 输出格式（严格 JSON）
{{
  "category": "分类名称",
  "confidence": 0.85,
  "reason": "简短理由（不超过30字）",
  "is_new_category": false
}}"""

_FEW_SHOT_EXAMPLES_CLOUD = """[示例 1]
文件名：2024Q3财务报告.xlsx
文件类型：xlsx
内容摘要：季度营收、利润、现金流数据...
{"category": "财务报表", "confidence": 0.95, "reason": "Excel财务数据文件，含营收利润", "is_new_category": false}"""

_USER_TEMPLATE = """请对以下文件进行分类：

## 文件信息
- 文件名：{file_name}
- 文件类型：{file_type}
- 文件大小：{file_size}
- 所在目录：{directory_path}
- 修改时间：{modified_time}

## 内容摘要（前 500 字符）
{content_summary}

## Few-shot 示例

{examples}

[待分类]
文件名：{file_name}
文件类型：{file_type}
内容摘要：{content_summary}"""


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


def _human_size(size_bytes: int) -> str:
    """字节数 → 人类可读大小（如 ``"2.3 MB"``），对齐 P-01 输入变量 ``file_size``。"""
    if size_bytes < 1024:
        return f"{size_bytes} B"
    kb = size_bytes / 1024
    if kb < 1024:
        return f"{kb:.1f} KB"
    mb = kb / 1024
    if mb < 1024:
        return f"{mb:.1f} MB"
    return f"{mb / 1024:.1f} GB"


def _file_type(item: ClassifyItem) -> str:
    """Prompt 用文件类型：扩展名，缺省时从文件名末尾猜测。"""
    if item.extension:
        return item.extension
    return item.name.rsplit(".", 1)[-1] if "." in item.name else ""


def build_classify_prompt(
    item: ClassifyItem,
    categories: list[str],
    masker: CloudMasker | None = None,
    version: PromptVersion = "local",
) -> tuple[str, str]:
    """按 P-01 模板构建 (system, user) 消息对。

    Args:
        item: 待分类文件信息。
        categories: 预定义分类列表（SQLite categories 表）。
        masker: 云端脱敏器。``None`` 且云端脱敏激活时自动创建（单文件兜底）；
            批处理场景由调用方传入共享实例，保证 ``file_001`` 编号跨文件连续。
        version: Prompt 版本（T8.5）。``"cloud"`` 用精简指令 + 单 few-shot
            （利用 response_format 强约束）；``"local"`` 用详细指令 + 3 few-shot。
            两者 JSON schema 完全一致。

    Returns:
        (system_prompt, user_prompt) 二元组，直接传入 LLM chat。
    """
    if masker is None and is_cloud_masking_active():
        masker = CloudMasker(content_max())

    if masker is not None:
        masked = masker.mask_file(item.path, item.name, item.path, item.content_summary)
        file_name = masked.mask_name
        directory_path = f"depth={masked.depth}"
        content_summary = masked.content_head
    else:
        file_name = item.name
        directory_path = item.path
        content_summary = item.content_summary[:CONTENT_SUMMARY_MAX]

    if version == "cloud":
        system_template, examples = _SYSTEM_TEMPLATE_CLOUD, _FEW_SHOT_EXAMPLES_CLOUD
    else:
        system_template, examples = _SYSTEM_TEMPLATE, _FEW_SHOT_EXAMPLES

    system = system_template.format(predefined_categories="、".join(categories))
    user = _USER_TEMPLATE.format(
        file_name=file_name,
        file_type=_file_type(item),
        file_size=_human_size(item.size),
        directory_path=directory_path,
        modified_time=item.modified_time,
        content_summary=content_summary,
        examples=examples,
    )
    return system, user


async def call_ollama_json(system: str, user: str) -> str:
    """调用 Ollama 生成严格 JSON 结果（P-01 分类 / P-02 查询改写共用）。

    连接/HTTP 失败抛 :class:`LLMUnavailableError`。``format="json"`` 约束输出，
    ``think=False`` 关闭 Qwen3 系列模型的思维链：思考 token 计入
    ``num_predict`` 预算，未关闭时 JSON 会被截断/报 502（非思维模型忽略此参数）。
    """
    client = AsyncClient(host=OLLAMA_HOST)
    try:
        resp = await client.chat(
            model=LLM_MODEL,
            messages=[
                Message(role="system", content=system),
                Message(role="user", content=user),
            ],
            format="json",
            think=False,
            options=Options(num_predict=256, temperature=0.0),
        )
    except (httpx.HTTPError, ResponseError) as exc:
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
