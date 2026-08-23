"""三层分类漏斗编排：规则引擎 → 启发式 → LLM 兜底。

对齐 08_Prompt工程设计 §7 三层分类漏斗与 04_API详细规格书 §3.2 POST /classify。
逐文件短路：规则命中 confidence=1.0；启发式命中 confidence=0.9；
LLM 兜底取模型置信度。LLM 并发上限 5，Ollama 不可用时跳过整个第 3 层。
"""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import TYPE_CHECKING

from app.core.cloud_mask import CloudMasker, content_max, is_cloud_masking_active
from app.core.logging import getLogger
from app.models import ClassifyItem, ClassifyResponse
from app.rules.engine import RuleEngine
from app.rules.heuristic import classify_heuristic
from app.rules.llm_classify import (
    CONFIDENCE_THRESHOLD,
    LLMUnavailableError,
    classify_file_with_llm,
)
from app.rules.models import FileMeta

if TYPE_CHECKING:
    from app.rules.engine import RuleMatch
    from app.rules.heuristic import HeuristicMatch
    from app.rules.llm_classify import ClassifyResult

logger = getLogger("filemind.classify")

#: 预置规则 JSON 路径（与 T4.1 一致）
PRESET_RULES_PATH = Path(__file__).resolve().parents[1] / "rules" / "presets" / "preset_rules.json"
#: LLM 并发上限：避免 Ollama 内存溢出（对齐 08-§2「并发分类请求限制为 5」）
MAX_CONCURRENT_LLM = 5
#: 规则层置信度（对齐 API 规格书「规则层为 1.0」）
RULE_CONFIDENCE = 1.0
#: 启发式层置信度（介于规则层与 LLM 之间，高于阈值判定为已分类）
HEURISTIC_CONFIDENCE = 0.9

#: 展示用状态枚举
STATUS_CLASSIFIED = "classified"
STATUS_NEEDS_REVIEW = "needs_review"
STATUS_UNCLASSIFIED = "unclassified"

#: 未知分类名（LLM 解析失败 / 超时 / 未分类兜底）
CATEGORY_UNCLASSIFIED = "未分类"


def build_default_engine() -> RuleEngine:
    """构建加载预置规则的默认引擎。

    每次请求全新加载，天然支持规则热更新（读取当前 JSON 文件）。
    """
    engine = RuleEngine(PRESET_RULES_PATH)
    engine.load()
    return engine


def _to_file_meta(item: ClassifyItem) -> FileMeta:
    """ClassifyItem → 规则/启发式层所需的 FileMeta（extension 规范化小写无点）。"""
    return FileMeta(
        name=item.name,
        extension=item.extension.lower().lstrip("."),
        path=Path(item.path),
        size=item.size,
    )


def _rule_item(item: ClassifyItem, rule_match: RuleMatch) -> dict[str, object]:
    """规则层命中结果 → 展示项（confidence=1.0）。"""
    return {
        "file_name": item.name,
        "category": rule_match.category,
        "confidence": RULE_CONFIDENCE,
        "status": STATUS_CLASSIFIED,
        "method": "rule",
        "reason": f"规则命中：{rule_match.rule_name}",
        "is_new_category": False,
    }


def _heuristic_item(item: ClassifyItem, heuristic: HeuristicMatch) -> dict[str, object]:
    """启发式层命中结果 → 展示项（confidence=0.9）。"""
    return {
        "file_name": item.name,
        "category": heuristic.category,
        "confidence": HEURISTIC_CONFIDENCE,
        "status": STATUS_CLASSIFIED,
        "method": "heuristic",
        "reason": f"启发式命中：{heuristic.method}",
        "is_new_category": False,
    }


def _llm_item(item: ClassifyItem, result: ClassifyResult) -> dict[str, object]:
    """LLM 兜底结果 → 展示项；低置信度标记 ``needs_review``（待人工确认）。"""
    if result.category == CATEGORY_UNCLASSIFIED:
        status = STATUS_UNCLASSIFIED
    elif result.confidence < CONFIDENCE_THRESHOLD:
        status = STATUS_NEEDS_REVIEW
    else:
        status = STATUS_CLASSIFIED
    return {
        "file_name": item.name,
        "category": result.category,
        "confidence": result.confidence,
        "status": status,
        "method": "llm",
        "reason": result.reason,
        "is_new_category": result.is_new_category,
    }


def _unclassified_item(item: ClassifyItem, reason: str) -> dict[str, object]:
    """未分类兜底展示项（未走 LLM / 第 3 层不可用）。"""
    return {
        "file_name": item.name,
        "category": CATEGORY_UNCLASSIFIED,
        "confidence": 0.0,
        "status": STATUS_UNCLASSIFIED,
        "method": "none",
        "reason": reason,
        "is_new_category": False,
    }


def _avg_confidence(results: list[dict[str, object]]) -> float:
    """所有文件置信度均值（异常值忽略）。"""
    values = [
        float(v) for v in (item["confidence"] for item in results) if isinstance(v, (int, float))
    ]
    return sum(values) / len(values) if values else 0.0


def _count_status(results: list[dict[str, object]], status: str) -> int:
    """统计指定状态的文件数。"""
    return sum(1 for item in results if item["status"] == status)


def _to_response(results: list[dict[str, object]]) -> ClassifyResponse:
    """聚合各文件结果 → ClassifyResponse（items/stats/平均置信度）。"""
    counts: dict[str, int] = {}
    for item in results:
        category = item["category"]
        counts[str(category)] = counts.get(str(category), 0) + 1
    stats: dict[str, object] = {
        "total": len(results),
        "classified": _count_status(results, STATUS_CLASSIFIED),
        "needs_review": _count_status(results, STATUS_NEEDS_REVIEW),
        "unclassified": _count_status(results, STATUS_UNCLASSIFIED),
        "categories": counts,
    }
    return ClassifyResponse(
        items=results, stats=stats, confidence=round(_avg_confidence(results), 4)
    )


async def _run_llm_pass(
    files: list[ClassifyItem],
    indices: list[int],
    categories: list[str],
    results: list[dict[str, object]],
) -> None:
    """对需 LLM 兜底的文件并发分类；Ollama 不可用时跳过整个第 3 层。

    并发受 ``MAX_CONCURRENT_LLM`` 信号量限制。``llm_down`` 一旦置位，
    其余任务不再发起调用，统一标记"待手动分类（LLM 不可用）"。
    """
    semaphore = asyncio.Semaphore(MAX_CONCURRENT_LLM)
    llm_down = False
    # 云端脱敏：批内共享一个 masker，保证 file_001.. 编号跨文件连续且可还原
    masker = CloudMasker(content_max()) if is_cloud_masking_active() else None

    async def classify_one(index: int) -> None:
        nonlocal llm_down
        if llm_down:
            results[index] = _unclassified_item(files[index], "待手动分类（LLM 不可用）")
            return
        async with semaphore:
            try:
                result = await classify_file_with_llm(files[index], categories, masker)
            except LLMUnavailableError as exc:
                llm_down = True
                logger.warning("llm.unavailable", error=str(exc))
                results[index] = _unclassified_item(files[index], "待手动分类（LLM 不可用）")
                return
        results[index] = _llm_item(files[index], result)

    await asyncio.gather(*(classify_one(index) for index in indices))


async def classify_files(
    files: list[ClassifyItem],
    categories: list[str],
    engine: RuleEngine | None = None,
) -> ClassifyResponse:
    """对文件列表执行三层分类漏斗（规则 → 启发式 → LLM），返回聚合响应。

    Args:
        files: 待分类文件列表。
        categories: 预定义分类列表（LLM 兜底用）。
        engine: 规则引擎；``None`` 时跳过规则层（测试/无预置集场景）。

    Returns:
        聚合响应：items（逐文件结果）、stats（分类统计）、confidence（均值）。
    """
    results: list[dict[str, object]] = []
    llm_indices: list[int] = []

    for index, item in enumerate(files):
        meta = _to_file_meta(item)
        rule_match = engine.match_file(meta) if engine is not None else None
        if rule_match is not None:
            results.append(_rule_item(item, rule_match))
            continue
        heuristic = classify_heuristic(meta)
        if heuristic is not None:
            results.append(_heuristic_item(item, heuristic))
            continue
        results.append(_unclassified_item(item, "待 LLM 兜底分类"))
        llm_indices.append(index)

    if llm_indices:
        await _run_llm_pass(files, llm_indices, categories, results)

    return _to_response(results)
