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
from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Sequence

    from sentence_transformers import CrossEncoder

#: Rerank 模型名（env 可覆盖；BAAI cross-encoder，首次下载后本地缓存）
RERANK_MODEL = os.environ.get("FILEMIND_RERANK_MODEL", "BAAI/bge-reranker-v2-m3")
#: HF 主站被墙（curl 000），模型下载走镜像；setdefault 不覆盖用户显式配置
os.environ.setdefault("HF_ENDPOINT", "https://hf-mirror.com")
#: 默认离线加载（用户可显式置 0 恢复联网）：模型已完整缓存在本地
#: (~/.cache/huggingface)，而打包后二进制经镜像下载会触发 TLS 握手失败
#: （SSLV3_ALERT_BAD_RECORD_MAC，主站又不可达）。离线模式直接从缓存加载，
#: 实测 ~5.8s 就绪；侧车仅此一处用 transformers，不影响 Ollama 链路的 embedding/LLM。
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

#: 单例 pipeline（懒加载；在 to_thread 中构造）
_pipeline: CrossEncoder | None = None
_pipeline_lock = asyncio.Lock()


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
    若顶层导入会让整个 sidecar 进程常驻超内存预算（Go/No-Go 第 7 项 <300MB）。
    """
    from sentence_transformers import CrossEncoder

    # sentence_transformers 已提供 py.typed，但 CrossEncoder 构造器/返回类型
    # 仍解析为 Any（下游 torch 缺 stub），mypy no-any-return 下需显式断言
    return cast("CrossEncoder", CrossEncoder(model))


async def _get_pipeline(model: str) -> CrossEncoder:
    """懒加载并返回 CrossEncoder 单例（并发下只构造一次）。

    Raises:
        RerankUnavailableError: 模型加载失败（未下载 / 依赖缺失）。
    """
    global _pipeline
    if _pipeline is not None:
        return _pipeline
    async with _pipeline_lock:
        if _pipeline is None:
            try:
                _pipeline = await asyncio.to_thread(_load_cross_encoder, model)
            except Exception as exc:  # noqa: BLE001
                raise RerankUnavailableError(f"Rerank 模型加载失败: {model!r}（{exc}）") from exc
    return _pipeline


async def rerank(
    query: str,
    candidates: Sequence[RerankCandidate],
    model: str = RERANK_MODEL,
    top_k: int = 5,
) -> list[RerankResult]:
    """对候选执行 cross-encoder 重排序，返回 Top-``top_k``。

    Args:
        query: 用户检索查询（自然语言）。
        candidates: 待重排候选（含原文 text；通常来自 RRF 融合 Top-20）。
        model: Rerank 模型名（默认 ``RERANK_MODEL``）。
        top_k: 精选候选数（默认 5，对齐「重排序 → Top-5」）。

    Returns:
        按相关分降序的 Top-``top_k`` 命中；空候选返回空列表。

    Raises:
        RerankUnavailableError: 模型加载或推理失败。
    """
    if not candidates:
        return []
    pipeline = await _get_pipeline(model)
    pairs = [(query, c.text) for c in candidates]
    try:
        raw_scores = await asyncio.to_thread(pipeline.predict, pairs)
    except Exception as exc:  # noqa: BLE001
        raise RerankUnavailableError(f"Rerank 推理失败: {exc}") from exc
    scored = [(float(score), cand) for score, cand in zip(raw_scores, candidates, strict=False)]
    scored.sort(key=lambda item: item[0], reverse=True)
    return [
        RerankResult(
            chunk_id=cand.chunk_id,
            score=score,
            file_path=cand.file_path,
            page=cand.page,
        )
        for score, cand in scored[:top_k]
    ]
