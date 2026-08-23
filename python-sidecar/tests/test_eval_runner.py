"""T4.5 — 评估 runner 单元测试（mock LLM，不走网络）。

用「oracle」LLM 兜底：对每个文件直接返回其正确分类，验证三层漏斗 + 指标
计算的端到端正确性；并验证 ``--no-llm`` 模式（规则 + 启发式两层）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import ClassifyResult  # noqa: E402
from eval.dataset import generate_dataset  # noqa: E402
from eval.runner import run_eval  # noqa: E402

if TYPE_CHECKING:
    from app.models import ClassifyItem  # noqa: E402


@pytest.mark.asyncio
async def test_run_eval_with_oracle_llm_high_accuracy() -> None:
    """oracle LLM → 三层漏斗 + 指标端到端，准确率 > 0.9 且有 LLM 兜底。"""
    records = generate_dataset(120, seed=1)
    by_name = {record.file_name: record.correct_category for record in records}

    async def oracle(item: ClassifyItem, categories: list[str], masker=None) -> ClassifyResult:
        return ClassifyResult(
            category=by_name[item.name], confidence=0.9, reason="oracle", is_new_category=False
        )

    with mock.patch("app.services.classify_service.classify_file_with_llm", oracle):
        report = await run_eval(records, use_llm=True)
    assert report.total == 120
    assert report.accuracy > 0.9
    assert report.llm_count > 0
    assert report.llm_accuracy > 0.9


@pytest.mark.asyncio
async def test_run_eval_no_llm_skips_llm_layer() -> None:
    """--no-llm：LLM 层不启用，规则/启发式两层仍有较高覆盖。"""
    records = generate_dataset(120, seed=2)
    report = await run_eval(records, use_llm=False)
    assert report.total == 120
    assert report.llm_count == 0
    assert report.json_parse_rate == 0.0
    assert report.rule_coverage > 0.0
    assert report.accuracy > 0.9
