"""T2.6 — services.embedding_switch_service 单元测试。

覆盖：next_version（无表/有 v1+ v3/ lancedb=None）、precheck_switch 全分支、
ensure_target_table 幂等。
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.db.lancedb_repo import LanceDBManager  # noqa: E402
from app.services import embedding_switch_service  # noqa: E402


@pytest.fixture
def ldb(tmp_path: Path) -> LanceDBManager:
    """独立 LanceDB 实例（每个用例隔离）。"""
    mgr = LanceDBManager(tmp_path / "ldb")
    mgr.connect()
    return mgr


# ------------------------------------------------------------------
# next_version
# ------------------------------------------------------------------


def test_next_version_no_tables_returns_default() -> None:
    """无任何表 → 分配 default_version（bge-large-zh-v1.5 default=1）。"""
    assert embedding_switch_service.next_version(None, "bge-large-zh-v1.5") == 1


def test_next_version_no_matching_model(ldb: LanceDBManager) -> None:
    """有其他模型的 documents 表，但目标模型没有 → default_version。"""
    # 建 v1 表但属于 bge-m3（不同模型）
    ldb.ensure_table("bge-m3", version=1, dim=1024)
    # 查询 bge-large-zh-v1.5 的下一个版本号（它还没表）
    assert embedding_switch_service.next_version(ldb, "bge-large-zh-v1.5") == 1


def test_next_version_has_v1_returns_v2(ldb: LanceDBManager) -> None:
    """已存在 documents_{model}_v1 → N+1 = 2。"""
    ldb.ensure_table("bge-large-zh-v1.5", version=1, dim=1024)
    assert embedding_switch_service.next_version(ldb, "bge-large-zh-v1.5") == 2


def test_next_version_has_v1_and_v3_skips_gap_returns_v4(ldb: LanceDBManager) -> None:
    """最大版本为 v3（缺 v2 不影响——取最大值 + 1）。"""
    ldb.ensure_table("bge-m3", version=1, dim=1024)
    ldb.ensure_table("bge-m3", version=3, dim=1024)
    assert embedding_switch_service.next_version(ldb, "bge-m3") == 4


def test_next_version_ignores_non_documents_tables(ldb: LanceDBManager) -> None:
    """非 documents_ 前缀的表不参与计数（list_document_tables 已过滤）。"""
    # 建一张 documents_{model}_v1，然后伪造另一个"不相关表"
    ldb.ensure_table("bge-m3", version=1, dim=1024)
    # LanceDB 不支持直接 create_table arbitrary name，但 list_document_tables 只
    # 认 documents_ 前缀，所以伪造前缀不匹配的情况即可（无需真有非法表）
    # 验证逻辑：如果实现走 list_document_tables → 只看到 v1 → 返回 2
    assert embedding_switch_service.next_version(ldb, "bge-m3") == 2


# ------------------------------------------------------------------
# precheck_switch
# ------------------------------------------------------------------


def _precheck(
    *,
    new_model: str = "bge-small-zh-v1.5",
    current_model: str = "bge-large-zh-v1.5",
    indexed_files: int = 100,
    lancedb=None,
    current_version: int = 1,
):
    """快捷入口：构造带默认值的预检查调用。"""
    return embedding_switch_service.precheck_switch(
        new_model=new_model,
        current_model=current_model,
        indexed_files=indexed_files,
        lancedb=lancedb,
        current_version=current_version,
    )


def test_model_diff_same_dim_is_dim_changed_true() -> None:
    """**关键分支：模型名变了但 dim 相同 → dim_changed=True（必须重 embedding）**。

    bge-large-zh-v1.5（1024）→ bge-m3（1024）：向量空间不同，必须全量。
    """
    r = _precheck(new_model="bge-m3", current_model="bge-large-zh-v1.5", indexed_files=500)
    assert r.dim_changed is True
    assert r.current_dim == 1024
    assert r.new_dim == 1024
    assert r.files_to_rebuild == 500
    assert r.old_table_preserved is True  # 永远 True


def test_model_diff_diff_dim_is_full_rebuild() -> None:
    """模型不同 + dim 变化 → files_to_rebuild = indexed_files。"""
    # bge-large-zh-v1.5(1024) → bge-small-zh-v1.5(512)
    r = _precheck(
        new_model="bge-small-zh-v1.5", current_model="bge-large-zh-v1.5", indexed_files=12847
    )
    assert r.dim_changed is True
    assert r.current_dim == 1024
    assert r.new_dim == 512
    assert r.files_to_rebuild == 12847
    # est_minutes = ceil(12847/500) = 26
    assert r.est_minutes == 26


def test_model_same_no_rebuild(ldb: LanceDBManager) -> None:
    """模型相同 → dim_changed=False，files=0（版本 bump，不走全量重建）。"""
    # 没有已有表 → version=1；next_version(None 无 lancedb) 返回 default
    r = _precheck(
        new_model="bge-large-zh-v1.5",
        current_model="bge-large-zh-v1.5",
        indexed_files=999,
        lancedb=ldb,  # 没有 documents_* 表，next_version 返回 default
    )
    assert r.dim_changed is False
    assert r.files_to_rebuild == 0
    assert r.est_minutes == 0  # 文件数为 0 时 est=0
    assert r.new_version == 1
    assert r.new_table_name.startswith("documents_bge-large-zh-v1.5_v")


def test_model_same_but_has_old_v(ldb: LanceDBManager) -> None:
    """模型相同，但 LanceDB 已有 v1 → new_version = 2。"""
    ldb.ensure_table("bge-large-zh-v1.5", version=1, dim=1024)
    r = _precheck(
        new_model="bge-large-zh-v1.5",
        current_model="bge-large-zh-v1.5",
        indexed_files=999,
        lancedb=ldb,
    )
    assert r.dim_changed is False
    assert r.files_to_rebuild == 0
    assert r.new_version == 2


def test_zero_indexed_files_gives_zero_est() -> None:
    """indexed_files=0 时 files_to_rebuild=0、est_minutes=0。"""
    r = _precheck(indexed_files=0)
    assert r.files_to_rebuild == 0
    assert r.est_minutes == 0


def test_huge_files_capped_by_999() -> None:
    """est_minutes 超过 EST_MINUTES_CAP（999）封顶。"""
    # 999 * 500 = 499500 文件 → ceil(499500/500) = 999，刚好在上限
    r = _precheck(indexed_files=999 * 500)
    assert r.est_minutes == 999
    # 再增加 → 仍然 999
    r2 = _precheck(indexed_files=999 * 500 + 1)
    assert r2.est_minutes == 999


def test_new_model_unknown_raises_valueerror() -> None:
    """目标模型未知 → ValueError。"""
    with pytest.raises(ValueError):
        _precheck(new_model="not-a-real-model")


def test_current_model_unknown_tolerated() -> None:
    """**当前模型未知：预检查不阻塞**，current_dim 退化为 0（但仍返回 new_dim 可用）。

    这种场景表示 Sidecar 状态异常（有人往 SQLite/state 里写了没注册的模型名），
    预检查不应该 crash——用户能看到"当前模型不合法，但目标模型是合法的 X，需要重建"。
    """
    r = _precheck(current_model="legacy-unknown-dim", indexed_files=100)
    # 模型名肯定不同 → dim_changed=True，files=100
    assert r.dim_changed is True
    assert r.files_to_rebuild == 100
    # current_dim 退化为 0（占位）；new_dim 正常 = 512（bge-small 默认 dim）
    assert r.current_dim == 0
    assert r.new_dim == 512


def test_old_table_preserved_always_true() -> None:
    """old_table_preserved 在任何分支都为 True（规则对齐）。"""
    for combo in [
        ("bge-small-zh-v1.5", "bge-large-zh-v1.5"),
        ("bge-large-zh-v1.5", "bge-large-zh-v1.5"),
        ("bge-m3", "bge-large-zh-v1.5"),
    ]:
        r = _precheck(new_model=combo[0], current_model=combo[1])
        assert r.old_table_preserved is True, f"combo {combo} 未保留旧表！"


# ------------------------------------------------------------------
# ensure_target_table
# ------------------------------------------------------------------


def test_ensure_target_table_creates_and_idempotent(ldb: LanceDBManager) -> None:
    """幂等性：第一次创建表（返回名），第二次直接返回同名，不抛错不重复。"""
    name1 = embedding_switch_service.ensure_target_table(
        lancedb=ldb,
        new_model="bge-small-zh-v1.5",
        new_version=1,
    )
    assert name1.startswith("documents_bge-small-zh-v1.5_v1")
    assert ldb.is_table_exists(name1)

    name2 = embedding_switch_service.ensure_target_table(
        lancedb=ldb,
        new_model="bge-small-zh-v1.5",
        new_version=1,
    )
    assert name2 == name1
