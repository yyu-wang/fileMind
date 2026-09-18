"""P-01 分类兜底 Prompt 构建：模板 / 变量填充 / 云端脱敏接入。

来源：08_Prompt工程设计 §2（P-01 分类兜底 Prompt）。
（2026-09-18 按职责拆自 llm_classify.py，355 行超 Python 警告阈值 300；
Ollama 调用与 JSON 解析仍在 :mod:`app.rules.llm_classify`。）

本地 / 云端两个提示词版本（T8.5）：
- ``local``：详细指令 + 3 few-shot，配合 Ollama ``format="json"``
- ``cloud``：精简指令 + 1 few-shot，利用云端 ``response_format(json_object)``
  强约束

两者 JSON schema 完全一致（DoD「同一请求在 local/cloud 下输出格式一致」）。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.core.cloud_mask import CloudMasker, content_max, is_cloud_masking_active

if TYPE_CHECKING:
    from app.models import ClassifyItem
    from app.services.cloud_provider import PromptVersion

#: 内容摘要截断长度（对齐 P-01 输入变量 content_summary 前 500 字符）
CONTENT_SUMMARY_MAX = 500

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
