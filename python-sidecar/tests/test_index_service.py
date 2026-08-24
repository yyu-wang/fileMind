"""T2.3 — index_service.evaluate_incremental 单元测试。

覆盖 API 规格书双重判断的全部分支：
  1. deleted → deleted 列表
  2. stored_hash=None（新文件）→ indexed
  3. content_hash != stored_hash（内容变了）→ indexed
  4. embedding_version != stored_embedding_version（模型变了）→ indexed
  5. 两者都没变 → skipped
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.index_service import (  # noqa: E402
    IncrementalChange,
    evaluate_incremental,
)

CURRENT_VERSION = 1


def _mk(
    file_id: str,
    change_type: str = "modified",
    content_hash: str = "aaa",
    stored_hash: str | None = "aaa",
    stored_embedding_version: int | None = 1,
) -> IncrementalChange:
    """构造单条 IncrementalChange，默认值模拟"无变化"场景。"""
    return IncrementalChange(
        file_id=file_id,
        file_path=f"/tmp/{file_id}",
        change_type=change_type,
        content_hash=content_hash,
        stored_hash=stored_hash,
        stored_embedding_version=stored_embedding_version,
    )


def test_deleted_goes_to_deleted() -> None:
    """change_type=deleted → deleted 列表。"""
    changes = [_mk("f1", change_type="deleted")]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert r.deleted == ["f1"]
    assert r.indexed == []
    assert r.skipped == []


def test_new_file_stored_hash_none_goes_to_indexed() -> None:
    """stored_hash=None → 新文件 → indexed。"""
    changes = [_mk("f2", stored_hash=None, stored_embedding_version=None)]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert r.indexed == ["f2"]
    assert r.skipped == []
    assert r.deleted == []


def test_content_hash_changed_goes_to_indexed() -> None:
    """content_hash != stored_hash → 内容变了 → indexed。"""
    changes = [_mk("f3", content_hash="new_hash", stored_hash="old_hash")]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert r.indexed == ["f3"]
    assert r.skipped == []


def test_embedding_version_changed_goes_to_indexed() -> None:
    """embedding_version != stored_embedding_version → 模型变了 → indexed。"""
    changes = [_mk("f4", stored_embedding_version=2)]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert r.indexed == ["f4"]
    assert r.skipped == []


def test_both_unchanged_goes_to_skipped() -> None:
    """hash 和 embedding_version 都没变 → skipped。"""
    changes = [_mk("f5")]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert r.skipped == ["f5"]
    assert r.indexed == []


def test_mixed_batch() -> None:
    """混合批次：1 deleted + 1 new + 1 content_changed + 1 version_changed + 1 skip。"""
    changes = [
        _mk("d1", change_type="deleted"),
        _mk("n1", stored_hash=None, stored_embedding_version=None),
        _mk("c1", content_hash="new", stored_hash="old"),
        _mk("v1", stored_embedding_version=99),
        _mk("s1"),  # both unchanged
    ]
    r = evaluate_incremental(changes, CURRENT_VERSION)
    assert sorted(r.deleted) == ["d1"]
    assert sorted(r.indexed) == ["c1", "n1", "v1"]
    assert sorted(r.skipped) == ["s1"]


def test_empty_changes() -> None:
    """空 changes → 全部空列表。"""
    r = evaluate_incremental([], CURRENT_VERSION)
    assert r.indexed == []
    assert r.skipped == []
    assert r.deleted == []
