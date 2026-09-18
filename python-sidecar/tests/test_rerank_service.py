"""T5.4 — rerank_service 单元测试。

覆盖：cross-encoder 打分排序 + top_k、元数据透传、空候选、懒加载单例、
加载/推理失败、HF_ENDPOINT 镜像兜底。均 mock CrossEncoder，不发真实推理。
"""

from __future__ import annotations

import asyncio
import os
import sys
import time
from pathlib import Path
from unittest import mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

import app.services.rerank_service as rerank_service  # noqa: E402
from app.services.rerank_service import (  # noqa: E402
    RERANK_MODEL,
    RerankCandidate,
    RerankResult,
    RerankUnavailableError,
    _get_pipeline,
    rerank,
)


def _reset_pipeline() -> None:
    """重置懒加载单例全局，隔离测试。"""
    rerank_service._pipeline = None
    rerank_service._pipeline_lock = asyncio.Lock()
    rerank_service._last_used_monotonic = None


class _FakeCrossEncoder:
    """可配置打分的假 CrossEncoder。"""

    def __init__(self, model: str, scores: list[float] | None = None) -> None:
        self.model = model
        self.scores = scores or [0.9, 0.8, 0.7]

    def predict(self, pairs: list[tuple[str, str]], **kwargs: object) -> list[float]:
        self.pairs = pairs
        return self.scores


def _pipeline_of(fake: object):
    async def get_pipeline(model: str) -> object:
        return fake

    return get_pipeline


# ------------------------------------------------------------------
# rerank 打分排序
# ------------------------------------------------------------------


async def test_rerank_scores_sorts_and_top_k() -> None:
    """按相关分降序取 Top-k，透传元数据，pair 为 (query, text)。"""
    candidates = [
        RerankCandidate("c1", "文本一", score=0.1, file_path="/a", page=1),
        RerankCandidate("c2", "文本二", score=0.2, file_path="/b", page=2),
        RerankCandidate("c3", "文本三", score=0.3, file_path="/c", page=3),
    ]
    fake = _FakeCrossEncoder("m", scores=[0.5, 0.9, 0.3])
    with mock.patch.object(rerank_service, "_get_pipeline", _pipeline_of(fake)):
        results = await rerank("查询", candidates, top_k=2)

    assert [r.chunk_id for r in results] == ["c2", "c1"]
    assert isinstance(results[0], RerankResult)
    assert results[0].score == pytest.approx(0.9)
    assert results[0].file_path == "/b"
    assert results[0].page == 2
    assert fake.pairs == [("查询", "文本一"), ("查询", "文本二"), ("查询", "文本三")]


async def test_rerank_empty_candidates() -> None:
    """空候选 → 空列表，不触发模型加载。"""

    async def fail_if_called(model: str) -> object:
        raise AssertionError("不应加载模型")

    with mock.patch.object(rerank_service, "_get_pipeline", fail_if_called):
        assert await rerank("查询", []) == []


# ------------------------------------------------------------------
# 懒加载单例
# ------------------------------------------------------------------


async def test_pipeline_lazy_singleton_constructed_once(monkeypatch: pytest.MonkeyPatch) -> None:
    """连续两次 rerank → CrossEncoder 只构造一次。"""
    # 本用例断言「默认加载目标 = HF 仓库 id」，故固定本地副本为不存在：
    # 真实机器上模型可能已下载到 `{models_root}/bge-reranker-v2-m3`，
    # 此时默认目标会是本地目录（该分支由 test_resolve_load_target_* 覆盖）
    monkeypatch.setattr(rerank_service, "model_ready", lambda _model: False)
    _reset_pipeline()
    constructs: list[str] = []

    def fake_load(model: str) -> _FakeCrossEncoder:
        constructs.append(model)
        return _FakeCrossEncoder(model)

    candidates = [RerankCandidate("c1", "文本", score=0.0)]
    with mock.patch.object(rerank_service, "_load_cross_encoder", fake_load):
        await rerank("q", candidates)
        await rerank("q", candidates)

    assert constructs == ["BAAI/bge-reranker-v2-m3"]


# ------------------------------------------------------------------
# 加载目标解析（本地副本优先）
# ------------------------------------------------------------------


def test_resolve_load_target_env_override_wins(monkeypatch: pytest.MonkeyPatch) -> None:
    """FILEMIND_RERANK_MODEL 非空 → 直接采用（兼容自带模型的部署）。"""
    monkeypatch.setenv("FILEMIND_RERANK_MODEL", "/custom/rerank-model")
    assert rerank_service.resolve_load_target() == "/custom/rerank-model"


def test_resolve_load_target_falls_back_to_repo(monkeypatch: pytest.MonkeyPatch) -> None:
    """本地缺失且无 env 覆盖 → 回退 HF 仓库 id。

    「本地副本就绪 → 用本地目录」这一分支需要真实模型目录，见
    ``test_model_download_rerank.py::test_load_target_prefers_local_copy``。
    """
    monkeypatch.delenv("FILEMIND_RERANK_MODEL", raising=False)
    monkeypatch.setattr(rerank_service, "model_ready", lambda _model: False)
    assert rerank_service.resolve_load_target() == "BAAI/bge-reranker-v2-m3"


# ------------------------------------------------------------------
# T10.3 空闲卸载（内存优先兜底）
# ------------------------------------------------------------------


async def test_idle_below_threshold_keeps_pipeline() -> None:
    """空闲未超阈值 → 复用已加载 pipeline，不重建。"""
    _reset_pipeline()
    fake = _FakeCrossEncoder("m")
    rerank_service._pipeline = fake
    rerank_service._last_used_monotonic = time.monotonic() - 1  # 1s 前刚用过

    def fail_if_reload(model: str) -> object:
        raise AssertionError("不应触发重建")

    with mock.patch.object(rerank_service, "_load_cross_encoder", fail_if_reload):
        pipeline = await _get_pipeline("BAAI/bge-reranker-v2-m3")

    assert pipeline is fake


async def test_idle_over_threshold_reloads_pipeline() -> None:
    """空闲超阈值 → 下次取用触发卸载并重建（懒加载语义保持）。"""
    _reset_pipeline()
    stale = _FakeCrossEncoder("stale")
    rerank_service._pipeline = stale
    rerank_service._last_used_monotonic = time.monotonic() - rerank_service.RERANK_IDLE_UNLOAD - 1
    constructs: list[str] = []

    def fake_load(model: str) -> _FakeCrossEncoder:
        constructs.append(model)
        return _FakeCrossEncoder(model)

    with mock.patch.object(rerank_service, "_load_cross_encoder", fake_load):
        pipeline = await _get_pipeline("BAAI/bge-reranker-v2-m3")

    assert constructs == ["BAAI/bge-reranker-v2-m3"]  # 已重建
    assert pipeline is not stale


async def test_rerank_bumps_last_used_timestamp() -> None:
    """成功推理后记录最后使用时间（空闲卸载判定的数据源）。"""
    _reset_pipeline()
    candidates = [RerankCandidate("c1", "文本", score=0.0)]
    fake = _FakeCrossEncoder("m")

    with mock.patch.object(rerank_service, "_get_pipeline", _pipeline_of(fake)):
        await rerank("q", candidates)

    assert rerank_service._last_used_monotonic is not None
    assert time.monotonic() - rerank_service._last_used_monotonic < 5


# ------------------------------------------------------------------
# 失败路径
# ------------------------------------------------------------------


async def test_pipeline_load_failure_raises() -> None:
    """模型加载失败 → RerankUnavailableError。"""
    _reset_pipeline()

    def failing(model: str) -> object:
        raise RuntimeError("model not downloaded")

    with (
        mock.patch.object(rerank_service, "_load_cross_encoder", failing),
        pytest.raises(RerankUnavailableError, match="加载失败"),
    ):
        await _get_pipeline("some-model")


async def test_rerank_inference_failure_raises() -> None:
    """推理失败 → RerankUnavailableError。"""

    class _Broken:
        def predict(self, pairs: list[tuple[str, str]], **kwargs: object) -> object:
            raise RuntimeError("broken")

    candidates = [RerankCandidate("c1", "文本", score=0.0)]
    with (
        mock.patch.object(rerank_service, "_get_pipeline", _pipeline_of(_Broken())),
        pytest.raises(RerankUnavailableError, match="推理失败"),
    ):
        await rerank("q", candidates)


# ------------------------------------------------------------------
# 配置常量
# ------------------------------------------------------------------


def test_rerank_model_default() -> None:
    """默认模型为 BAAI cross-encoder。"""
    assert RERANK_MODEL == "BAAI/bge-reranker-v2-m3"


def test_hf_endpoint_defaults_to_mirror() -> None:
    """模块导入时 HF_ENDPOINT 兜底到镜像（主站被墙）。"""
    assert os.environ.get("HF_ENDPOINT") == "https://hf-mirror.com"
