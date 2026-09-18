"""T5.6 — P-03 检索增强生成提示词构建：上下文块格式化 + (system, user) 组装。

来源：08_Prompt工程设计 §4（P-03 rag_prompt.py）。
（2026-09-18 按职责拆自 generation_service.py，349 行超 Python 警告阈值 300；
流式调用与引用解析仍在 :mod:`app.services.generation_service`。）

检索片段（``SourceChunk``）格式化为带 ``[N]`` 引用编号的上下文块，再按本地 /
云端模板组装消息对。P-04 自纠正复用同一 ``format_context_blocks``，保证验证
与生成基于同一事实依据。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.core.cloud_mask import CloudMasker, content_max, is_cloud_masking_active
from app.services.provider_factory import truncate_context

if TYPE_CHECKING:
    from app.services.cloud_provider import PromptVersion

#: P-03 单个检索片段内容截断长度（对齐输入变量 source_N_content「限 500 字符」）
CONTENT_MAX = 500

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
