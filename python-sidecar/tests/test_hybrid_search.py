"""T5.3 — 混合检索单元测试。

覆盖：RRF 融合（UT-PY-002 门控 k=60）、RRF 公式与空边界、真实 LanceDB
向量检索（tmp_path 建表，无 Ollama 依赖）、hybrid_search 编排（mock 向量化）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.db.lancedb_repo import LanceDBManager, VectorHit  # noqa: E402
from app.services.embedding_service import QUERY_INSTRUCTION  # noqa: E402
from app.services.hybrid_search import FusedHit, hybrid_search, rrf_fusion  # noqa: E402

# ------------------------------------------------------------------
# RRF 融合（UT-PY-002 门控）
# ------------------------------------------------------------------


def test_rrf_fusion_ut_py_002_gate() -> None:
    """向量 [A,B,C,D] + FTS [B,D,E,F]，k=60 → B、D 排名高于 E、F。"""
    fused = rrf_fusion([["A", "B", "C", "D"], ["B", "D", "E", "F"]], k=60)
    ids = [h.chunk_id for h in fused]
    assert ids[0] == "B"
    assert ids[1] == "D"
    assert ids.index("E") > ids.index("B")
    assert ids.index("F") > ids.index("D")


def test_rrf_fusion_dual_hit_ranks_above_single() -> None:
    """两路都命中的 chunk 融合分高于仅一路命中。"""
    by_id = {h.chunk_id: h.rrf_score for h in rrf_fusion([["A", "B"], ["B"]], k=60)}
    assert by_id["B"] > by_id["A"]


def test_rrf_fusion_tie_break_by_chunk_id() -> None:
    """同分（C 与 E 均 1/63）→ 按 chunk_id 升序（确定性）。"""
    ids = [h.chunk_id for h in rrf_fusion([["A", "B", "C", "D"], ["B", "D", "E", "F"]])]
    assert ids.index("C") < ids.index("E")


# ------------------------------------------------------------------
# RRF 公式与空边界
# ------------------------------------------------------------------


def test_rrf_fusion_single_list_score() -> None:
    """单列表首元素：score == 1/(k+1)。"""
    fused = rrf_fusion([["A"]], k=60)
    assert fused == [FusedHit(chunk_id="A", rrf_score=1.0 / 61.0)]


def test_rrf_fusion_preserves_single_list_order() -> None:
    """仅一路结果 → 顺序不变。"""
    assert [h.chunk_id for h in rrf_fusion([["x", "y", "z"]], k=60)] == ["x", "y", "z"]


def test_rrf_fusion_empty_inputs() -> None:
    """空输入 → 空结果；空列表不贡献任何分。"""
    assert rrf_fusion([]) == []
    assert rrf_fusion([[], []]) == []
    assert rrf_fusion([["A"], []]) == [FusedHit(chunk_id="A", rrf_score=1.0 / 61.0)]


# ------------------------------------------------------------------
# LanceDB 向量检索（真实库，tmp_path 建表）
# ------------------------------------------------------------------


def _seed_vector_table(manager: LanceDBManager, table_name: str) -> None:
    """建 3 行向量：c0 最近、c2 次近、c1 最远（L2 距离互不相同）。"""
    manager.ensure_table("bge-large-zh-v1.5", 1, dim=3)
    tbl = manager.open_table(table_name)
    tbl.add(
        [
            {
                "vector": [1.0, 0.0, 0.0],
                "chunk_id": "c0",
                "file_path": "/a.txt",
                "chunk_text": "文本0",
                "page": 1,
            },
            {
                "vector": [0.5, 0.5, 0.0],
                "chunk_id": "c2",
                "file_path": "/c.txt",
                "chunk_text": "文本2",
                "page": 0,
            },
            {
                "vector": [0.0, 1.0, 0.0],
                "chunk_id": "c1",
                "file_path": "/b.txt",
                "chunk_text": "文本1",
                "page": 2,
            },
        ]
    )


def test_search_vectors_orders_by_distance(tmp_path: Path) -> None:
    """查询 [1,0,0] → 命中按 L2 距离升序（c0, c2, c1）且字段映射正确。"""
    manager = LanceDBManager(db_path=tmp_path / "lancedb")
    manager.connect()
    table_name = "documents_bge-large-zh-v1.5_v1"
    _seed_vector_table(manager, table_name)

    hits = manager.search_vectors(table_name, [1.0, 0.0, 0.0], top_k=3)

    assert [h.chunk_id for h in hits] == ["c0", "c2", "c1"]
    assert hits[0].file_path == "/a.txt"
    assert hits[0].chunk_text == "文本0"
    assert hits[0].page == 1
    assert hits[0].distance < hits[1].distance < hits[2].distance


def test_search_vectors_respects_top_k(tmp_path: Path) -> None:
    """top_k 截断命中数。"""
    manager = LanceDBManager(db_path=tmp_path / "lancedb")
    manager.connect()
    table_name = "documents_bge-large-zh-v1.5_v1"
    _seed_vector_table(manager, table_name)

    hits = manager.search_vectors(table_name, [1.0, 0.0, 0.0], top_k=2)

    assert [h.chunk_id for h in hits] == ["c0", "c2"]


def test_search_vectors_missing_table_returns_empty(tmp_path: Path) -> None:
    """表不存在（尚未建索引）→ 空列表，混合检索退化为纯 FTS 排序。"""
    manager = LanceDBManager(db_path=tmp_path / "lancedb")
    manager.connect()
    assert manager.search_vectors("documents_bge-large-zh-v1.5_v1", [1.0, 0.0]) == []


# ------------------------------------------------------------------
# hybrid_search 编排（mock 向量化 + mock 向量检索）
# ------------------------------------------------------------------


def _fake_manager(vector_hits: list[VectorHit]) -> mock.MagicMock:
    manager = mock.MagicMock()
    manager.search_vectors.return_value = vector_hits
    return manager


async def test_hybrid_search_fuses_and_enriches() -> None:
    """向量 [chunk_a, chunk_b] + FTS [chunk_b, chunk_c] → chunk_b 居首 + 元数据回填。"""
    vector_hits = [
        VectorHit(chunk_id="chunk_a", file_path="/a", chunk_text="文本a", page=1, distance=0.1),
        VectorHit(chunk_id="chunk_b", file_path="/b", chunk_text="文本b", page=2, distance=0.2),
    ]
    manager = _fake_manager(vector_hits)
    embed_calls: list[str] = []

    async def fake_embed(text: str, model: str = "") -> list[float]:
        embed_calls.append(text)
        return [1.0, 0.0]

    with mock.patch("app.services.hybrid_search.embed_text", fake_embed):
        fused = await hybrid_search(
            "测试查询",
            ["chunk_b", "chunk_c"],
            manager,
            "documents_bge-large-zh-v1.5_v1",
            top_k=2,
        )

    assert embed_calls == [QUERY_INSTRUCTION + "测试查询"]
    manager.search_vectors.assert_called_once()
    assert fused[0].chunk_id == "chunk_b"

    by_id = {h.chunk_id: h for h in fused}
    assert by_id["chunk_a"].file_path == "/a"
    assert by_id["chunk_a"].chunk_text == "文本a"
    assert by_id["chunk_a"].page == 1
    assert by_id["chunk_c"].file_path == ""
    assert by_id["chunk_c"].chunk_text == ""
    assert by_id["chunk_c"].page == 0


async def test_hybrid_search_degrades_to_fts_when_no_table() -> None:
    """向量路为空（表不存在）→ 融合结果 = 纯 FTS 顺序。"""
    manager = _fake_manager([])

    async def fake_embed(text: str, model: str = "") -> list[float]:
        return [1.0, 0.0]

    with mock.patch("app.services.hybrid_search.embed_text", fake_embed):
        fused = await hybrid_search("查询", ["first", "second"], manager, "documents_nope_v1")

    assert [h.chunk_id for h in fused] == ["first", "second"]
