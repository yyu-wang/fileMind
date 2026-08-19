"""T4.5 — 评估指标计算单元测试。

覆盖：分类归一化（细粒度 → 评估分类）、准确率、混淆矩阵、
按类别精确率/召回率/F1、端到端 evaluate 聚合。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from eval.dataset import generate_dataset  # noqa: E402
from eval.metrics import (  # noqa: E402
    _per_category,
    accuracy,
    confusion_matrix,
    evaluate,
    normalize_category,
)


def test_normalize_category_aliases() -> None:
    """细粒度分类归并到评估分类。"""
    assert normalize_category("文档") == "文档"
    assert normalize_category("办公文档") == "文档"
    assert normalize_category("财务") == "文档"
    assert normalize_category("合同") == "文档"
    assert normalize_category("表格") == "文档"
    assert normalize_category("按日期归档") == "文档"
    assert normalize_category("版本文件") == "文档"
    assert normalize_category("截图") == "图片"
    assert normalize_category("设计文件") == "设计"
    assert normalize_category("数据文件") == "其他"
    assert normalize_category("项目文件") == "其他"
    assert normalize_category("未分类") == "其他"
    assert normalize_category("不存在的分类") == "其他"


def test_normalize_passthrough() -> None:
    """8 个评估分类原样透传。"""
    for category in ("文档", "图片", "视频", "音频", "代码", "压缩包", "设计", "其他"):
        assert normalize_category(category) == category


def test_accuracy() -> None:
    """逐样本比对求准确率；空输入返回 0。"""
    assert accuracy(["a", "a", "b"], ["a", "a", "b"]) == 1.0
    assert accuracy(["a", "a", "b"], ["a", "b", "b"]) == 2 / 3
    assert accuracy([], []) == 0.0


def test_confusion_matrix_counts() -> None:
    """混淆矩阵正确计数，零单元补齐。"""
    labels = ("a", "b")
    cm = confusion_matrix(["a", "a", "b", "b"], ["a", "a", "a", "b"], labels)
    assert cm[("a", "a")] == 2
    assert cm[("b", "a")] == 1
    assert cm[("b", "b")] == 1
    assert cm[("a", "b")] == 0
    assert cm[("b", "b")] + cm[("a", "a")] + cm[("b", "a")] + cm[("a", "b")] == 4


def test_evaluate_perfect_and_metrics() -> None:
    """全部命中正确分类 → 准确率 1.0，覆盖率与指标合理。"""
    records = generate_dataset(20, seed=7)
    items = [
        {
            "file_name": record.file_name,
            "category": record.correct_category,
            "method": "rule",
            "reason": "",
        }
        for record in records
    ]
    report = evaluate(items, records)
    assert report.total == 20
    assert report.accuracy == 1.0
    assert report.rule_coverage == 1.0
    assert report.llm_count == 0
    assert report.json_parse_rate == 0.0
    for m in report.per_category:
        assert m.support >= 0
        assert 0.0 <= m.precision <= 1.0
        assert 0.0 <= m.recall <= 1.0
        assert 0.0 <= m.f1 <= 1.0


def test_evaluate_counts_llm_and_parse_failures() -> None:
    """LLM 兜底文件计数、准确率与 JSON 解析成功率正确统计。"""
    records = generate_dataset(20, seed=7)
    items = [
        {
            "file_name": record.file_name,
            "category": record.correct_category,
            "method": "llm" if index % 2 else "rule",
            # 仅奇数索引中每 4 个取 1 个解析失败 → 10 个 LLM 文件里 5 个失败
            "reason": "解析失败" if index % 4 == 3 else "",
        }
        for index, record in enumerate(records)
    ]
    report = evaluate(items, records)
    assert report.llm_count == 10
    assert report.llm_accuracy == 1.0
    assert report.json_parse_rate == 0.5


def test_per_category_precision_recall_f1() -> None:
    """按类别指标：真实 [a,a,b,b] 预测 [a,a,a,b]。
    对 a: tp=2, fp=1, fn=0 → precision=2/3, recall=1.0, f1=0.8。
    对 b: tp=1, fp=0, fn=1 → precision=1.0, recall=0.5, f1=2/3。
    """
    cm = confusion_matrix(["a", "a", "b", "b"], ["a", "a", "a", "b"], ("a", "b"))
    per = _per_category(cm, ("a", "b"))
    a_metrics, b_metrics = per
    assert a_metrics.support == 2
    assert a_metrics.precision == 2 / 3
    assert a_metrics.recall == 1.0
    assert a_metrics.f1 == 0.8
    assert b_metrics.support == 2
    assert b_metrics.precision == 1.0
    assert b_metrics.recall == 0.5
    assert b_metrics.f1 == 2 / 3
