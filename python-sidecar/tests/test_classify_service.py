"""T4.4 — 三层分类漏斗编排单元测试（mock LLM，不走网络）。

覆盖：规则/启发式短路、LLM 兜底、低置信度标记、Ollama 不可用跳过整个
第 3 层、并发限流、聚合统计。
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path
from unittest import mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.models import ClassifyItem  # noqa: E402
from app.rules.llm_classify import ClassifyResult, LLMUnavailableError  # noqa: E402
from app.services.classify_service import (  # noqa: E402
    MAX_CONCURRENT_LLM,
    build_default_engine,
    classify_files,
)


def make_item(
    name: str = "report.pdf",
    extension: str = "pdf",
    path: str = "/data",
    size: int = 1000,
) -> ClassifyItem:
    """构造待分类文件。"""
    return ClassifyItem(name=name, extension=extension, path=path, size=size)


def _fail_if_called(*args: object, **kwargs: object) -> ClassifyResult:
    """LLM 被调用即失败（用于验证规则/启发式短路）。"""
    raise AssertionError("LLM 不应被调用")


@pytest.mark.asyncio
async def test_rule_layer_wins_skips_llm() -> None:
    """规则命中（report.pdf → 文档）→ method=rule，LLM 不调用。"""
    engine = build_default_engine()
    with mock.patch("app.services.classify_service.classify_file_with_llm", _fail_if_called):
        resp = await classify_files([make_item("report.pdf")], [], engine=engine)
    item = resp.items[0]
    assert item["category"] == "文档"
    assert item["method"] == "rule"
    assert item["status"] == "classified"
    assert item["confidence"] == 1.0


@pytest.mark.asyncio
async def test_heuristic_layer_wins_skips_llm() -> None:
    """启发式命中（会议纪要.bkp → 文档）→ method=heuristic，LLM 不调用。"""
    with mock.patch("app.services.classify_service.classify_file_with_llm", _fail_if_called):
        resp = await classify_files([make_item("会议纪要.bkp", "bkp")], [])
    item = resp.items[0]
    assert item["category"] == "文档"
    assert item["method"] == "heuristic"
    assert item["confidence"] == 0.9


@pytest.mark.asyncio
async def test_llm_layer_classified() -> None:
    """两层未命中 → LLM 兜底，高置信度 status=classified。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        return ClassifyResult(
            category="数据文件", confidence=0.9, reason="未知类型", is_new_category=False
        )

    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files([make_item("weird.zzzz", "zzzz")], ["数据文件"])
    item = resp.items[0]
    assert item["category"] == "数据文件"
    assert item["method"] == "llm"
    assert item["status"] == "classified"
    assert item["confidence"] == 0.9


@pytest.mark.asyncio
async def test_llm_low_confidence_needs_review() -> None:
    """LLM 低置信度 → status=needs_review（待人工确认）。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        return ClassifyResult(
            category="数据文件", confidence=0.5, reason="不确定", is_new_category=False
        )

    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files([make_item("weird.zzzz", "zzzz")], [])
    item = resp.items[0]
    assert item["status"] == "needs_review"
    assert item["confidence"] == 0.5


@pytest.mark.asyncio
async def test_llm_unavailable_marks_manual() -> None:
    """Ollama 不可用 → 整个第 3 层跳过，文件标记待手动分类。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        raise LLMUnavailableError("Ollama down")

    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files([make_item("weird.zzzz", "zzzz")], [])
    item = resp.items[0]
    assert item["status"] == "unclassified"
    assert item["category"] == "未分类"
    assert "待手动分类" in str(item["reason"])


@pytest.mark.asyncio
async def test_llm_timeout_marks_unclassified() -> None:
    """单文件 LLM 超时 → 未分类，不阻塞其他文件。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        return ClassifyResult(
            category="未分类",
            confidence=0.0,
            reason="LLM 超时，已跳过该文件",
            is_new_category=False,
        )

    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files([make_item("weird.zzzz", "zzzz")], [])
    item = resp.items[0]
    assert item["category"] == "未分类"
    assert item["status"] == "unclassified"


@pytest.mark.asyncio
async def test_llm_concurrency_limited() -> None:
    """并发分类不超过 MAX_CONCURRENT_LLM。"""
    counters = {"active": 0, "max": 0}

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        counters["active"] += 1
        counters["max"] = max(counters["max"], counters["active"])
        await asyncio.sleep(0.02)
        counters["active"] -= 1
        return ClassifyResult(
            category="数据文件", confidence=0.9, reason="ok", is_new_category=False
        )

    files = [make_item(f"u{i}.zzz", "zzzz") for i in range(12)]
    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files(files, [])
    assert len(resp.items) == 12
    assert counters["max"] <= MAX_CONCURRENT_LLM


@pytest.mark.asyncio
async def test_empty_files_returns_empty_response() -> None:
    """空文件列表 → 空响应，confidence 0.0。"""
    resp = await classify_files([], [])
    assert resp.items == []
    assert resp.confidence == 0.0
    assert resp.stats["total"] == 0


@pytest.mark.asyncio
async def test_response_stats_counts_statuses() -> None:
    """stats 正确统计 classified/needs_review/unclassified 与分类分布。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        return ClassifyResult(
            category="数据文件", confidence=0.5, reason="不确定", is_new_category=False
        )

    files = [
        make_item("report.pdf"),  # 规则 → classified
        make_item("会议纪要.bkp", "bkp"),  # 启发式 → classified
        make_item("weird.zzzz", "zzzz"),  # LLM 低置信 → needs_review
    ]
    engine = build_default_engine()
    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files(files, [], engine=engine)
    assert resp.stats["total"] == 3
    assert resp.stats["classified"] == 2
    assert resp.stats["needs_review"] == 1
    assert resp.stats["unclassified"] == 0
    categories = resp.stats["categories"]
    assert isinstance(categories, dict)
    assert categories["文档"] == 2
    assert categories["数据文件"] == 1


@pytest.mark.asyncio
async def test_mixed_funnel_confidence_average() -> None:
    """平均置信度正确（1.0 + 0.9 + 0.9 → 0.9333）。"""

    async def fake_llm(
        item: ClassifyItem, categories: list[str], masker=None, provider=None
    ) -> ClassifyResult:
        return ClassifyResult(
            category="数据文件", confidence=0.9, reason="ok", is_new_category=False
        )

    files = [
        make_item("report.pdf"),
        make_item("会议纪要.bkp", "bkp"),
        make_item("weird.zzzz", "zzzz"),
    ]
    engine = build_default_engine()
    with mock.patch("app.services.classify_service.classify_file_with_llm", fake_llm):
        resp = await classify_files(files, [], engine=engine)
    assert resp.confidence == round((1.0 + 0.9 + 0.9) / 3, 4)
