"""T5.3 混合检索：向量检索 + FTS5 + RRF 融合排序。

链路（02_总体实施计划 §RAG 流水线）：
    FTS5 Top-20 + LanceDB 向量 Top-20 → RRF 融合排序 Top-20。

硬约束：Sidecar 永不碰 SQLite → FTS5 由 Rust 层执行，FTS 命中以
``fts_chunk_ids``（有序 chunk_id 列表，按 BM25 序）作为函数输入传入；
本模块只负责查询向量化 + LanceDB 向量检索 + RRF 融合。

RRF 公式（UT-PY-002 门控，k=60）：
    score(chunk) = Σ_lists 1 / (k + rank)
两路都命中的 chunk 因叠加加分而排名更高。
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.services.embedding_service import (
    EMBEDDING_MODEL,
    QUERY_INSTRUCTION,
    embed_text,
)

if TYPE_CHECKING:
    from collections.abc import Sequence

    from app.db.lancedb_repo import LanceDBManager


@dataclass(frozen=True)
class FusedHit:
    """RRF 融合后的命中项。"""

    chunk_id: str
    rrf_score: float
    file_path: str = ""  # 仅向量路命中可回填；FTS-only 命中留空（文本在 Rust 侧）
    chunk_text: str = ""
    page: int = 0


def rrf_fusion(ranked_lists: Sequence[Sequence[str]], k: int = 60) -> list[FusedHit]:
    """RRF 融合纯函数：叠加各路的 rank 倒数加分，按融合分降序返回。

    Args:
        ranked_lists: 各路有序结果（每路已按相关性降序；元素为 chunk_id）。
        k: RRF 平滑常数（默认 60，对齐 UT-PY-002）。

    Returns:
        按 ``rrf_score`` 降序的融合命中；同分按 chunk_id 升序（确定性排序）。
    """
    scores: dict[str, float] = {}
    for ranked in ranked_lists:
        for rank, chunk_id in enumerate(ranked, start=1):
            scores[chunk_id] = scores.get(chunk_id, 0.0) + 1.0 / (k + rank)
    ordered = sorted(scores.items(), key=lambda kv: (-kv[1], kv[0]))
    return [FusedHit(chunk_id=cid, rrf_score=score) for cid, score in ordered]


async def hybrid_search(
    query: str,
    fts_chunk_ids: Sequence[str],
    manager: LanceDBManager,
    table_name: str,
    model: str = EMBEDDING_MODEL,
    top_k: int = 20,
    k: int = 60,
) -> list[FusedHit]:
    """混合检索编排：查询向量化 → 向量检索 → 与 FTS 命中 RRF 融合。

    Args:
        query: 用户检索查询（自然语言）。
        fts_chunk_ids: Rust 层 FTS5 命中，按 BM25 相关性降序。
        manager: LanceDB 管理器（负责向量表检索）。
        table_name: 向量表名（``documents_{model}_v{version}``）。
        model: Embedding 模型名（默认注册表默认值）。
        top_k: 向量路截断候选数（默认 20）。
        k: RRF 常数（默认 60）。

    Returns:
        按 ``rrf_score`` 降序的融合命中；向量表不存在时退化为纯 FTS 排序。
    """
    # SC-m24：bge-large-zh 检索需加指令前缀（BAAI 模型卡要求），其他模型不需要
    instruction = QUERY_INSTRUCTION if model.startswith("bge-large") else ""
    query_vec = await embed_text(instruction + query, model=model)
    # SC-C2：ANN 检索是同步 CPU/IO 混合操作（表大后单次几十~几百 ms），
    # 下沉线程池避免卡住事件循环（影响并发请求与流式 token 节奏）
    vector_hits = await asyncio.to_thread(manager.search_vectors, table_name, query_vec, top_k)
    fused = rrf_fusion([[h.chunk_id for h in vector_hits], list(fts_chunk_ids)], k=k)
    meta = {h.chunk_id: h for h in vector_hits}
    enriched: list[FusedHit] = []
    for hit in fused:
        source = meta.get(hit.chunk_id)
        enriched.append(
            FusedHit(
                chunk_id=hit.chunk_id,
                rrf_score=hit.rrf_score,
                file_path=source.file_path if source else "",
                chunk_text=source.chunk_text if source else "",
                page=source.page if source else 0,
            )
        )
    return enriched
