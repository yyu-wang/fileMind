"""T4.5/T4.6 评估指标：分类归一化 + 混淆矩阵 + 准确率/覆盖率 + 达标校验。

对齐 08_Prompt工程设计 §8 评估指标：总体准确率 ≥85%、规则层覆盖率 ≥70%、
LLM 兜底准确率 ≥75%、JSON 解析成功率 ≥95%。
"""

from __future__ import annotations

from dataclasses import dataclass

from eval.dataset import CATEGORY_NAMES, EvalRecord

#: 08-§8 达标目标
TARGET_ACCURACY = 0.85
TARGET_RULE_COVERAGE = 0.70
TARGET_LLM_ACCURACY = 0.75
TARGET_JSON_PARSE_RATE = 0.95

#: 漏斗输出分类 → 8 个评估分类（细粒度分类归并；未知分类兜底到「其他」）
CATEGORY_NORMALIZE: dict[str, str] = {
    "文档": "文档",
    "办公文档": "文档",
    "财务": "文档",
    "合同": "文档",
    "简历": "文档",
    "表格": "文档",
    "按日期归档": "文档",
    "版本文件": "文档",
    "图片": "图片",
    "截图": "图片",
    "视频": "视频",
    "音频": "音频",
    "代码": "代码",
    "压缩包": "压缩包",
    "设计": "设计",
    "设计文件": "设计",
    "数据文件": "其他",
    "项目文件": "其他",
    "其他": "其他",
    "未分类": "其他",
}


def normalize_category(category: str) -> str:
    """将漏斗输出分类归并到评估分类；未知分类兜底为「其他」。"""
    return CATEGORY_NORMALIZE.get(category, "其他")


def accuracy(y_true: list[str], y_pred: list[str]) -> float:
    """逐样本比对求准确率；空列表返回 0.0。"""
    if not y_true:
        return 0.0
    correct = sum(1 for t, p in zip(y_true, y_pred, strict=False) if t == p)
    return correct / len(y_true)


def confusion_matrix(
    y_true: list[str], y_pred: list[str], labels: tuple[str, ...]
) -> dict[tuple[str, str], int]:
    """构造混淆矩阵（稀疏字典，(真实, 预测) → 计数），labels 中零计数也补齐。"""
    cm: dict[tuple[str, str], int] = {}
    for t, p in zip(y_true, y_pred, strict=False):
        key = (t, p)
        cm[key] = cm.get(key, 0) + 1
    for true in labels:
        for pred in labels:
            cm.setdefault((true, pred), 0)
    return cm


@dataclass(frozen=True)
class PerCategoryMetrics:
    """单分类的精确率 / 召回率 / F1。"""

    category: str
    support: int
    precision: float
    recall: float
    f1: float


def _per_category(
    cm: dict[tuple[str, str], int], labels: tuple[str, ...]
) -> list[PerCategoryMetrics]:
    """从混淆矩阵计算各分类指标。"""
    metrics: list[PerCategoryMetrics] = []
    for category in labels:
        tp = cm.get((category, category), 0)
        fp = sum(
            count for (true, pred), count in cm.items() if pred == category and true != category
        )
        fn = sum(
            count for (true, pred), count in cm.items() if true == category and pred != category
        )
        support = tp + fn
        precision = tp / (tp + fp) if tp + fp else 0.0
        recall = tp / (tp + fn) if tp + fn else 0.0
        f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
        metrics.append(
            PerCategoryMetrics(
                category=category,
                support=support,
                precision=precision,
                recall=recall,
                f1=f1,
            )
        )
    return metrics


@dataclass(frozen=True)
class EvalReport:
    """一次评估的汇总结果。"""

    total: int
    accuracy: float
    rule_coverage: float
    llm_count: int
    llm_accuracy: float
    json_parse_rate: float
    confusion: dict[tuple[str, str], int]
    per_category: list[PerCategoryMetrics]


def evaluate(items: list[dict[str, object]], records: list[EvalRecord]) -> EvalReport:
    """将逐文件分类结果与标注样本对齐，计算指标。

    Args:
        items: ``ClassifyResponse.items``（每个元素含 category/method/reason）。
        records: 标注样本列表（与 items 按下标一一对应）。

    Returns:
        汇总报告：总准确率、规则层覆盖率、LLM 兜底准确率、JSON 解析成功率等。
    """
    y_true = [normalize_category(record.correct_category) for record in records]
    y_pred: list[str] = []
    rule_hits = 0
    llm_true: list[str] = []
    llm_pred: list[str] = []
    parse_failures = 0
    for index, item in enumerate(items):
        pred = normalize_category(str(item.get("category", "未分类")))
        y_pred.append(pred)
        if item.get("method") == "rule":
            rule_hits += 1
        if item.get("method") == "llm":
            llm_true.append(y_true[index])
            llm_pred.append(pred)
            if "解析失败" in str(item.get("reason", "")):
                parse_failures += 1

    cm = confusion_matrix(y_true, y_pred, CATEGORY_NAMES)
    llm_count = len(llm_true)
    return EvalReport(
        total=len(records),
        accuracy=accuracy(y_true, y_pred),
        rule_coverage=rule_hits / len(records) if records else 0.0,
        llm_count=llm_count,
        llm_accuracy=accuracy(llm_true, llm_pred) if llm_count else 0.0,
        json_parse_rate=1.0 - parse_failures / llm_count if llm_count else 0.0,
        confusion=cm,
        per_category=_per_category(cm, CATEGORY_NAMES),
    )


def check_targets(report: EvalReport, min_accuracy: float = TARGET_ACCURACY) -> list[str]:
    """返回未达标指标描述列表；全部达标返回空列表。

    Args:
        report: 评估报告。
        min_accuracy: 总体准确率门禁（CLI ``--min-accuracy``，默认 0.85）。

    Returns:
        未达标项列表；LLM 未启用（``llm_count == 0``）时跳过 LLM 相关校验。
    """
    failed: list[str] = []
    if report.accuracy < min_accuracy:
        failed.append(f"总体准确率 {report.accuracy:.1%} < {min_accuracy:.0%}")
    if report.rule_coverage < TARGET_RULE_COVERAGE:
        failed.append(f"规则层覆盖率 {report.rule_coverage:.1%} < {TARGET_RULE_COVERAGE:.0%}")
    if report.llm_count and report.llm_accuracy < TARGET_LLM_ACCURACY:
        failed.append(f"LLM 兜底准确率 {report.llm_accuracy:.1%} < {TARGET_LLM_ACCURACY:.0%}")
    if report.llm_count and report.json_parse_rate < TARGET_JSON_PARSE_RATE:
        failed.append(
            f"JSON 解析成功率 {report.json_parse_rate:.1%} < {TARGET_JSON_PARSE_RATE:.0%}"
        )
    return failed


def format_report(report: EvalReport) -> str:
    """渲染评估报告为可读文本。"""
    lines = [
        "=== 分类准确率评估报告 ===",
        f"总样本数: {report.total}",
        f"总体准确率: {report.accuracy:.1%}",
        f"规则层覆盖率: {report.rule_coverage:.1%}",
        f"LLM 兜底文件数: {report.llm_count}",
        f"LLM 兜底准确率: {report.llm_accuracy:.1%}",
        f"JSON 解析成功率: {report.json_parse_rate:.1%}",
        "",
        "按类别指标 (precision/recall/f1):",
    ]
    for m in report.per_category:
        lines.append(
            f"  {m.category:<4} 支持={m.support:<4} "
            f"P={m.precision:.2f} R={m.recall:.2f} F1={m.f1:.2f}"
        )
    lines.append("")
    lines.append("混淆矩阵 (行=真实, 列=预测):")
    header = "         " + "".join(f"{c:<8}" for c in CATEGORY_NAMES)
    lines.append(header)
    for true in CATEGORY_NAMES:
        cells = "".join(f"{report.confusion.get((true, pred), 0):<8}" for pred in CATEGORY_NAMES)
        lines.append(f"{true:<8}" + cells)
    return "\n".join(lines)
