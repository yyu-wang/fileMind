"""T5.2 — eval.recall 单元测试。

覆盖：语料确定性、_cosine 数学、Top-k 召回计算（mock 聚簇/劣化向量，不走
真实 Ollama）、查询指令前缀、门禁退出码。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from eval.recall import (  # noqa: E402
    _TOPICS,
    QUERY_INSTRUCTION,
    RecallQuery,
    RecallReport,
    _cosine,
    generate_corpus,
    generate_queries,
    main,
    run_recall,
)  # noqa: I001


def _topic_index_for_doc(text: str) -> int:
    """按标题前缀反查文档所属主题索引（text = "财务报表文档1\\n..."）。"""
    for index, (_key, title, _sentences) in enumerate(_TOPICS):
        if text.startswith(f"{title}文档"):
            return index
    raise AssertionError(f"无法识别文档主题: {text[:20]!r}")


# ------------------------------------------------------------------
# 语料生成
# ------------------------------------------------------------------


def test_corpus_deterministic_and_shape() -> None:
    """同 seed → 相同；10 主题 × 5 文档 = 50 篇。"""
    assert generate_corpus(5, seed=42) == generate_corpus(5, seed=42)
    docs = generate_corpus(5, seed=42)
    assert len(docs) == 50
    topics = sorted({d.doc_id.rsplit("_doc", 1)[0] for d in docs})
    assert len(topics) == 10
    for topic in topics:
        assert sum(1 for d in docs if d.doc_id.startswith(f"{topic}_doc")) == 5


def test_queries_annotate_topic_docs() -> None:
    """每条查询标注 5 个相关文档，且均属同一主题。"""
    docs = generate_corpus(5, seed=42)
    queries = generate_queries(docs)
    assert len(queries) == 10
    for q in queries:
        assert len(q.relevant_ids) == 5
        topic = q.relevant_ids[0].rsplit("_doc", 1)[0]
        assert all(did.rsplit("_doc", 1)[0] == topic for did in q.relevant_ids)
        assert all(did in {d.doc_id for d in docs} for did in q.relevant_ids)


# ------------------------------------------------------------------
# _cosine
# ------------------------------------------------------------------


def test_cosine() -> None:
    """余弦相似度：同向=1、正交=0、反向=-1、零向量=0。"""
    assert _cosine([1.0, 0.0], [1.0, 0.0]) == 1.0
    assert _cosine([1.0, 0.0], [0.0, 1.0]) == 0.0
    assert _cosine([1.0, 0.0], [-1.0, 0.0]) == -1.0
    assert _cosine([0.0, 0.0], [1.0, 0.0]) == 0.0


# ------------------------------------------------------------------
# run_recall（mock 向量）
# ------------------------------------------------------------------


def _cluster_embedder():
    """按主题聚簇的假向量：文档贴近其标题所属主题，查询 j 贴近主题 j。"""
    query_vecs = iter([[1.0 if j == k else 0.0 for k in range(10)] for j in range(10)])

    async def fake_texts(texts: list[str], model: str = "") -> list[list[float]]:
        vectors = []
        for text in texts:
            topic = _topic_index_for_doc(text)
            offset = len(vectors) % 5
            vectors.append([1.0 + 0.01 * offset if k == topic else 0.0 for k in range(10)])
        return vectors

    async def fake_text(text: str, model: str = "") -> list[float]:
        return next(query_vecs)

    return fake_texts, fake_text


async def test_run_recall_perfect_clusters() -> None:
    """同主题文档向量贴近 + 查询贴近其主题簇 → 总体召回 1.0。"""
    docs = generate_corpus(5, seed=42)
    queries = generate_queries(docs)
    fake_texts, fake_text = _cluster_embedder()
    with (
        mock.patch("eval.recall.embed_texts", fake_texts),
        mock.patch("eval.recall.embed_text", fake_text),
    ):
        report = await run_recall(docs, queries, top_k=5, threshold=0.8)
    assert report.overall_recall == 1.0
    assert all(p.recall == 1.0 for p in report.per_query)
    assert all(p.hits == p.relevant_count for p in report.per_query)


async def test_run_recall_degraded_model() -> None:
    """查询贴近错误主题（劣化模型）→ 总体召回 0，未达标。"""
    docs = generate_corpus(5, seed=42)
    queries = generate_queries(docs)
    fake_texts, _ = _cluster_embedder()
    # 查询 j 落到 (j+1)%10 主题：每个查询都错过自己主题的全部文档
    wrong_query_vecs = iter(
        [[1.0 if k == (j + 1) % 10 else 0.0 for k in range(10)] for j in range(10)]
    )

    async def wrong_text(text: str, model: str = "") -> list[float]:
        return next(wrong_query_vecs)

    with (
        mock.patch("eval.recall.embed_texts", fake_texts),
        mock.patch("eval.recall.embed_text", wrong_text),
    ):
        report = await run_recall(docs, queries, top_k=5, threshold=0.8)
    assert report.overall_recall == 0.0
    assert report.overall_recall < 0.8


async def test_run_recall_query_instruction_prefix() -> None:
    """查询向量化收到指令前缀；文档不受影响。"""
    docs = generate_corpus(5, seed=42)
    queries = generate_queries(docs)
    calls: list[str] = []

    async def spy_texts(texts: list[str], model: str = "") -> list[list[float]]:
        calls.extend(texts)
        return [[1.0] * 10 for _ in texts]

    async def spy_text(text: str, model: str = "") -> list[float]:
        calls.append(text)
        return [1.0] * 10

    with (
        mock.patch("eval.recall.embed_texts", spy_texts),
        mock.patch("eval.recall.embed_text", spy_text),
    ):
        await run_recall(docs, queries, top_k=5)
    query_texts = [c for c in calls if c.startswith(QUERY_INSTRUCTION)]
    assert len(query_texts) == 10
    doc_texts = [c for c in calls if not c.startswith(QUERY_INSTRUCTION)]
    assert len(doc_texts) == 50


# ------------------------------------------------------------------
# CLI 门禁
# ------------------------------------------------------------------


def _report(overall_recall: float) -> RecallReport:
    """构造指定总体召回率的报告。"""
    return RecallReport(
        docs=(),
        queries=(),
        per_query=(),
        overall_recall=overall_recall,
        top_k=5,
        threshold=0.8,
    )


def test_main_exit_zero_when_above_threshold(monkeypatch) -> None:
    """总体召回 > 80% → 退出码 0。"""

    async def fake_run(*args: object, **kwargs: object) -> RecallReport:
        return _report(0.95)

    monkeypatch.setattr("eval.recall.run_recall", fake_run)
    assert main(["--seed", "42"]) == 0


def test_main_exit_one_when_below_threshold(monkeypatch) -> None:
    """总体召回 <= 80% → 退出码 1。"""

    async def fake_run(*args: object, **kwargs: object) -> RecallReport:
        return _report(0.5)

    monkeypatch.setattr("eval.recall.run_recall", fake_run)
    assert main(["--seed", "42"]) == 1


def test_generate_queries_types() -> None:
    """RecallQuery 字段类型正确（Pydantic-free 冻结 dataclass）。"""
    docs = generate_corpus(5, seed=42)
    queries = generate_queries(docs)
    q = queries[0]
    assert isinstance(q, RecallQuery)
    assert isinstance(q.relevant_ids, tuple)
    assert all(isinstance(did, str) for did in q.relevant_ids)
