"""批次 8 代码审查修复回归测试（SC-C1/C2/C3 + SC-M3/M5）。

- SC-C3：重复 build 不翻倍；file_id 互为前缀不误删（真实 tmp LanceDB）
- SC-M3：stored_embedding_version=None 进 indexed 而非 skipped
- SC-C1/C2：同步阻塞操作下沉 to_thread 后事件循环保持可调度（探针计数）
- SC-M5：/index/build 成功后查询缓存被清空
"""

from __future__ import annotations

import asyncio
import time
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest

from app.db.lancedb_repo import DocumentChunk, LanceDBManager  # noqa: E402
from app.services import ingest_service  # noqa: E402
from app.services.index_service import evaluate_incremental  # noqa: E402

if TYPE_CHECKING:
    from pathlib import Path

TABLE = "documents_bge-large-zh-v1.5_v1"


@pytest.fixture()
def ldb(tmp_path: Path) -> LanceDBManager:
    mgr = LanceDBManager(tmp_path / "lancedb_b8")
    mgr.connect()
    return mgr


def _chunk(chunk_id: str, dim: int = 2) -> DocumentChunk:
    return DocumentChunk(
        vector=[0.1] * dim,
        chunk_id=chunk_id,
        file_path="/tmp/x",
        chunk_text="内容",
        page=0,
    )


def _count_rows(mgr: LanceDBManager, table: str) -> int:
    return mgr.open_table(table).count_rows()


# ---------- SC-C3：重复索引不产生重复向量行 ----------


@pytest.mark.asyncio
async def test_build_twice_no_duplicate_rows(ldb: LanceDBManager, tmp_path: Path) -> None:
    """同一文件 build 两次：行数不翻倍、检索无重复 chunk_id（SC-C3）。"""
    ldb.ensure_table("bge-large-zh-v1.5", 1, 2)
    txt = tmp_path / "note.md"
    txt.write_text("第一段\n\n第二段", encoding="utf-8")

    async def fake_embed(texts: list[str], model: str) -> list[list[float]]:
        return [[0.1, 0.2] for _ in texts]

    files = [("f1", str(txt))]
    with patch.object(ingest_service, "embed_texts", side_effect=fake_embed):
        r1 = await ingest_service.build_index(files, TABLE, ldb)
        r2 = await ingest_service.build_index(files, TABLE, ldb)

    assert r1.indexed == 1 and r2.indexed == 1
    # 两次 build 后行数仍等于单次分块数（无追加翻倍）
    assert _count_rows(ldb, TABLE) == 2
    # 检索无重复 chunk_id
    hits = ldb.search_vectors(TABLE, [0.1, 0.2], top_k=100)
    ids = [h.chunk_id for h in hits]
    assert len(ids) == len(set(ids)) == 2


def test_delete_chunks_exact_prefix_no_cross_delete(ldb: LanceDBManager) -> None:
    """file_id 互为前缀（abc / abc-1）不互相误删（regexp 精确匹配回归）。"""
    ldb.ensure_table("bge-large-zh-v1.5", 1, 2)
    ldb.add_chunks(TABLE, [_chunk("abc-0"), _chunk("abc-1-0"), _chunk("abd-0")])

    # 删除 abc 的行：只应删掉 abc-0，不能带走 abc-1-0
    ldb.delete_chunks_by_file_id(TABLE, "abc")

    hits = ldb.search_vectors(TABLE, [0.1, 0.1], top_k=100)
    ids = {h.chunk_id for h in hits}
    assert "abc-0" not in ids
    assert "abc-1-0" in ids
    assert "abd-0" in ids


def test_delete_chunks_invalid_file_id_rejected(ldb: LanceDBManager) -> None:
    """白名单拒绝：引号/空串抛 ValueError，不执行删除。"""
    ldb.ensure_table("bge-large-zh-v1.5", 1, 2)
    with pytest.raises(ValueError, match="非法 file_id"):
        ldb.delete_chunks_by_file_id(TABLE, "a'b")
    with pytest.raises(ValueError, match="非法 file_id"):
        ldb.delete_chunks_by_file_id(TABLE, "")


# ---------- SC-M3：embedding_version=None 进待索引集 ----------


def test_none_embedding_version_counts_as_indexed() -> None:
    """stored_embedding_version=None（索引过但从未向量化）→ indexed。"""
    change = SimpleNamespace(
        change_type="modified",
        file_id="f1",
        content_hash="h1",
        stored_hash="h1",  # hash 一致（否则规则 2 先命中）
        stored_embedding_version=None,
    )
    result = evaluate_incremental([change], current_embedding_version="v1")
    assert result.indexed == ["f1"]
    assert result.skipped == []


# ---------- SC-C1/C2：to_thread 下沉后事件循环不被阻塞 ----------


async def _run_with_probe(coro_factory):  # noqa: ANN001, ANN202
    """事件循环探针：await 目标协程期间自旋计数 asyncio.sleep(0)。

    同步阻塞会把探针一起卡住（tick 数骤降）；下沉线程池后应持续高频调度。
    """
    ticks = 0

    async def probe() -> None:
        nonlocal ticks
        while True:
            await asyncio.sleep(0)
            ticks += 1

    probe_task = asyncio.create_task(probe())
    try:
        await coro_factory()
    finally:
        probe_task.cancel()
        await asyncio.gather(probe_task, return_exceptions=True)
    return ticks


@pytest.mark.asyncio
async def test_build_index_does_not_block_event_loop(tmp_path: Path) -> None:
    """SC-C1：读+写盘各 mock 50ms 同步阻塞，期间事件循环探针仍持续调度。"""
    txt = tmp_path / "note.md"
    txt.write_text("第一段\n\n第二段", encoding="utf-8")

    calls: list[str] = []

    def slow_add(table_name: str, chunks) -> int:  # noqa: ANN001
        time.sleep(0.05)
        calls.append("add")
        return len(chunks)

    def slow_delete(table_name: str, file_id: str) -> int:
        time.sleep(0.05)
        calls.append("del")
        return 0

    mgr = SimpleNamespace(
        add_chunks=slow_add,
        delete_chunks_by_file_id=slow_delete,
    )

    async def fake_embed(texts: list[str], model: str) -> list[list[float]]:
        return [[0.1, 0.2] for _ in texts]

    async def run_build() -> None:
        with patch.object(ingest_service, "embed_texts", side_effect=fake_embed):
            await ingest_service.build_index([("f1", str(txt))], TABLE, mgr)  # type: ignore[arg-type]

    ticks = await _run_with_probe(run_build)
    # 100ms 的同步阻塞期间探针仍持续被调度（to_thread 生效）
    assert ticks >= 10, f"事件循环被阻塞：probe 仅调度 {ticks} 次"
    assert "add" in calls and "del" in calls


@pytest.mark.asyncio
async def test_search_vectors_does_not_block_event_loop() -> None:
    """SC-C2：同步 ANN 检索 mock 50ms，hybrid_search 期间探针持续调度。"""
    from app.services import hybrid_search as hs

    def slow_search(table_name: str, vec: list[float], top_k: int = 20):  # noqa: ANN001
        time.sleep(0.05)
        return []

    mgr = SimpleNamespace(search_vectors=slow_search)

    async def fake_embed_one(text: str, model: str = "") -> list[float]:
        return [0.1] * 8

    async def run_search() -> None:
        with patch.object(hs, "embed_text", side_effect=fake_embed_one):
            hits = await hs.hybrid_search("查询", ["c1"], mgr, TABLE)  # type: ignore[arg-type]
        # 向量路空、FTS 路 c1 命中 → 融合结果仅 c1（纯 FTS 排序退化）
        assert [h.chunk_id for h in hits] == ["c1"]

    ticks = await _run_with_probe(run_search)
    assert ticks >= 10, f"事件循环被阻塞：probe 仅调度 {ticks} 次"


# ---------- SC-M5：build 后查询缓存清空 ----------


async def _fake_embed_texts(texts: list[str], model: str) -> list[list[float]]:
    return [[0.1, 0.2] for _ in texts]


@pytest.mark.asyncio
async def test_build_clears_query_cache(tmp_path: Path) -> None:
    """build 路由成功后 get_query_cache() 被 clear（直接调 async 路由函数，
    绕过 HTTP/HMAC 层——SC-M5 验证的是路由逻辑而非传输层）。"""
    from app.api.routes_index import build_index as route_build
    from app.models import IndexBuildRequest
    from app.services.query_cache import get_query_cache, reset_query_cache

    reset_query_cache()
    try:
        cache = get_query_cache()
        # 预填一条旧缓存（模拟 60s TTL 内的过期检索结果）
        await cache.set(("k",), ("重写", 1, [], []))  # type: ignore[arg-type]
        assert len(cache) == 1

        txt = tmp_path / "note.md"
        txt.write_text("第一段", encoding="utf-8")

        mgr = SimpleNamespace(
            ensure_table=lambda *a, **k: TABLE,
            add_chunks=lambda t, c: len(c),
            delete_chunks_by_file_id=lambda t, f: 0,
        )
        req = IndexBuildRequest(
            files=[
                __import__("app.models", fromlist=["IndexBuildFile"]).IndexBuildFile(
                    file_id="f1", path=str(txt)
                )
            ],
            embedding_model="bge-large-zh-v1.5",
            table_name=TABLE,
        )
        with (
            patch("app.state.get_lancedb", return_value=mgr),
            patch.object(ingest_service, "embed_texts", side_effect=_fake_embed_texts),
        ):
            resp = await route_build(req)
        assert resp.indexed_count == 1
        assert len(cache) == 0, "build 成功后查询缓存应被清空（SC-M5）"
    finally:
        reset_query_cache()
