"""T5.4 BGE-reranker 重排序服务（cross-encoder 精排 Top-20 → Top-5）。

链路（02_总体实施计划 §4.5）：
    RRF 融合 Top-20 → BGE-reranker 重排序 → 精选 Top-5 → LLM 生成。

实现：sentence-transformers 的 CrossEncoder（cross-encoder 需 query+doc
逐对打分，无法用 bi-encoder 的独立向量代替）。Ollama 不提供 /api/rerank
（本地 404 验证 + 社区 PR 未合并），故直接本地加载模型；HF 主站被墙，
模型下载走镜像。

并发：模型懒加载单例，加载与推理均在 ``asyncio.to_thread`` 中执行，
避免阻塞事件循环。
"""

from __future__ import annotations

import asyncio
import os
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

from app.core.logging import getLogger
from app.services.model_download_service import model_ready
from app.services.model_specs import RERANK_MODEL_NAME, RERANK_REPO, model_dir_for

if TYPE_CHECKING:
    from collections.abc import Sequence

    from sentence_transformers import CrossEncoder

logger = getLogger("filemind.rerank")

#: Rerank 仓库 id：模型下载源，也是本地副本缺失时的兜底加载目标
RERANK_MODEL = RERANK_REPO
#: HF 主站被墙（curl 000），模型下载走镜像；setdefault 不覆盖用户显式配置
os.environ.setdefault("HF_ENDPOINT", "https://hf-mirror.com")
#: 默认离线加载（用户可显式置 0 恢复联网）：加载目标优先用本应用下载到
#: ``{models_root}/bge-reranker-v2-m3`` 的本地副本（见 :func:`resolve_load_target`），
#: 离线开关对它无影响；只有回退到 HF 仓库 id 时才需要 HF 缓存——而打包后经镜像
#: 下载会触发 TLS 握手失败（SSLV3_ALERT_BAD_RECORD_MAC，主站又不可达）。因此
#: 「本地副本缺失」的正确解法是在设置页下载模型，而不是指望联网兜底。
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")


def resolve_load_target() -> str:
    """解析模型的加载目标：env 覆盖 → 本地已下载目录 → HF 仓库 id。

    本地目录优先：全新机器没有 HuggingFace 缓存、hf-mirror 又可能不可达，
    而模型可由设置页触发下载到 ``{models_root}/bge-reranker-v2-m3``
    （:mod:`app.services.model_download_service`）。从本地目录加载纯离线，
    实测冷加载约 20s（torch 导入占大头），不依赖任何远端。

    ``FILEMIND_RERANK_MODEL`` 非空时直接采用（兼容自行准备模型的部署与快速回退）。
    """
    override = os.environ.get("FILEMIND_RERANK_MODEL", "").strip()
    if override:
        return override
    if model_ready(RERANK_MODEL_NAME):
        return str(model_dir_for(RERANK_MODEL_NAME))
    return RERANK_MODEL


#: 单例 pipeline（懒加载；在 to_thread 中构造）
_pipeline: CrossEncoder | None = None
_pipeline_lock = asyncio.Lock()
#: 空闲卸载阈值（秒，env 可覆盖；默认 600s = 10min 无推理即释放模型，
#: 内存优先取舍的兜底：防 rerank 长时间常驻占用内存预算）
RERANK_IDLE_UNLOAD = float(os.environ.get("FILEMIND_RERANK_IDLE_UNLOAD", "600") or "600")
#: 上次 rerank 推理结束的 monotonic 时间戳（None = 尚未推理过）
_last_used_monotonic: float | None = None


@dataclass(frozen=True)
class RerankCandidate:
    """待重排候选（通常来自 RRF 融合 Top-20）。"""

    chunk_id: str
    text: str  # 重排用原文片段（调用方保证非空）
    score: float  # 预重排分（RRF 融合分），并列参考
    file_path: str = ""
    page: int = 0


@dataclass(frozen=True)
class RerankResult:
    """重排后的命中项。"""

    chunk_id: str
    score: float  # cross-encoder 相关分（v2-m3 sigmoid → [0,1]，越高越相关）
    file_path: str = ""
    page: int = 0


class RerankUnavailableError(Exception):
    """Rerank 模型加载/推理失败（模型未下载、torch 缺失等）。"""


def _load_cross_encoder(model: str) -> CrossEncoder:
    """构造 CrossEncoder。

    延迟导入 sentence-transformers：torch 加载即占用约 500MB+ RSS，
    若顶层导入会让整个 sidecar 进程常驻超内存预算（Go/No-Go 第 7 项 <500MB）。
    """
    from sentence_transformers import CrossEncoder

    # sentence_transformers 已提供 py.typed，但 CrossEncoder 构造器/返回类型
    # 仍解析为 Any（下游 torch 缺 stub），mypy no-any-return 下需显式断言
    return cast("CrossEncoder", CrossEncoder(model))


def _unload_if_idle() -> None:
    """已加载模型空闲超过阈值 → 释放（内存优先取舍的兜底，非预载路径）。

    只在下次取用 pipeline 时惰性检查：不引入定时器，避免空转；卸载后
    下次推理会重新加载（懒加载语义不变）。
    """
    global _pipeline, _last_used_monotonic
    if (
        _pipeline is not None
        and _last_used_monotonic is not None
        and time.monotonic() - _last_used_monotonic >= RERANK_IDLE_UNLOAD
    ):
        _pipeline = None
        _last_used_monotonic = None


async def _get_pipeline(model: str) -> CrossEncoder:
    """懒加载并返回 CrossEncoder 单例（并发下只构造一次）。

    Raises:
        RerankUnavailableError: 模型加载失败（未下载 / 依赖缺失）。
    """
    global _pipeline
    _unload_if_idle()
    if _pipeline is not None:
        return _pipeline
    async with _pipeline_lock:
        if _pipeline is None:
            logger.info("rerank.load.start", target=model)
            started = time.monotonic()
            try:
                _pipeline = await asyncio.to_thread(_load_cross_encoder, model)
            except Exception as exc:  # noqa: BLE001
                raise RerankUnavailableError(f"Rerank 模型加载失败: {model!r}（{exc}）") from exc
            # 记录耗时：冷加载（torch 导入 + 模型构造）期间事件循环会被拖住，
            # 该毫秒数与 Rust watchdog 观测到的 /health 无响应窗口相互印证
            # （阈值含义见 src-tauri/src/sidecar/manager/mod.rs）。
            logger.info(
                "rerank.load.done", target=model, ms=int((time.monotonic() - started) * 1000)
            )
    return _pipeline


async def rerank(
    query: str,
    candidates: Sequence[RerankCandidate],
    model: str | None = None,
    top_k: int = 5,
) -> list[RerankResult]:
    """对候选执行 cross-encoder 重排序，返回 Top-``top_k``。

    Args:
        query: 用户检索查询（自然语言）。
        candidates: 待重排候选（含原文 text；通常来自 RRF 融合 Top-20）。
        model: 加载目标；``None``（默认）表示按 :func:`resolve_load_target` 解析，
            即「env 覆盖 → 本地已下载目录 → HF 仓库 id」。
        top_k: 精选候选数（默认 5，对齐「重排序 → Top-5」）。

    Returns:
        按相关分降序的 Top-``top_k`` 命中；空候选返回空列表。

    Raises:
        RerankUnavailableError: 模型加载或推理失败。
    """
    global _last_used_monotonic
    if not candidates:
        return []
    pipeline = await _get_pipeline(model or resolve_load_target())
    pairs = [(query, c.text) for c in candidates]
    try:
        raw_scores = await asyncio.to_thread(pipeline.predict, pairs)
    except Exception as exc:  # noqa: BLE001
        raise RerankUnavailableError(f"Rerank 推理失败: {exc}") from exc
    # T10.3：记录最后使用时间（空闲卸载判定；推理失败则不计入使用）
    _last_used_monotonic = time.monotonic()
    scored = [(float(score), cand) for score, cand in zip(raw_scores, candidates, strict=False)]
    # SC-m25：cross-encoder 对近似等距候选可能产出浮点精度差异（±1e-8），
    # 再加上 Python sort stable 依赖上游顺序，显式加 chunk_id 升序作为
    # tie-breaker，确保重排结果全确定性。
    scored.sort(key=lambda item: (-item[0], item[1].chunk_id))
    return [
        RerankResult(
            chunk_id=cand.chunk_id,
            score=score,
            file_path=cand.file_path,
            page=cand.page,
        )
        for score, cand in scored[:top_k]
    ]
