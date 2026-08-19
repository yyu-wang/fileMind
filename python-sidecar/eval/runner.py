"""T4.5 评估编排：把标注样本喂给三层分类漏斗并计算指标。

``run_eval`` 接受 ``use_llm`` 开关：为 ``False`` 时通过 mock 让 LLM 层
抛 ``LLMUnavailableError``，从而只评估规则 + 启发式两层（对应 ``--no-llm``）。
"""

from __future__ import annotations

from unittest import mock

from app.models import ClassifyItem
from app.rules.llm_classify import LLMUnavailableError
from app.services.classify_service import build_default_engine, classify_files
from eval.dataset import CATEGORY_NAMES, EvalRecord, parse_size
from eval.metrics import EvalReport, evaluate


def _to_classify_item(record: EvalRecord) -> ClassifyItem:
    """EvalRecord → 待分类的 ClassifyItem（extension 规范化小写无点）。"""
    return ClassifyItem(
        name=record.file_name,
        extension=record.file_type.lstrip("."),
        path=record.path,
        size=parse_size(record.size),
        content_summary=record.content_summary,
        modified_time=record.modified_time,
    )


def _llm_unavailable(*args: object, **kwargs: object) -> object:
    """``--no-llm`` 模式下代替 LLM 调用，强制跳过整个第 3 层。"""
    raise LLMUnavailableError("LLM 已禁用（--no-llm）")


async def run_eval(records: list[EvalRecord], use_llm: bool = True) -> EvalReport:
    """对样本集执行三层分类漏斗并计算指标。

    Args:
        records: 带标注样本列表。
        use_llm: 是否启用 LLM 兜底层；``False`` 时仅测规则 + 启发式两层。

    Returns:
        评估报告（准确率、覆盖率、混淆矩阵等）。
    """
    items = [_to_classify_item(record) for record in records]
    engine = build_default_engine()
    if use_llm:
        response = await classify_files(items, list(CATEGORY_NAMES), engine=engine)
    else:
        with mock.patch("app.services.classify_service.classify_file_with_llm", _llm_unavailable):
            response = await classify_files(items, list(CATEGORY_NAMES), engine=engine)
    return evaluate(response.items, records)
