"""T2.2 — LanceDBManager 单元测试。

用 tempfile.TemporaryDirectory 隔离测试数据；每个用例独立 LanceDB 实例。
"""

from __future__ import annotations

import os
import stat
import sys
from pathlib import Path

import pytest

# python-sidecar 作为根包导入
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.db.lancedb_repo import LanceDBManager  # noqa: E402


@pytest.fixture
def ldb(tmp_path: Path) -> LanceDBManager:
    """每个用例独立的 LanceDB 管理器（tmp_path 自带隔离）。"""
    mgr = LanceDBManager(tmp_path / "lancedb_test")
    mgr.connect()
    return mgr


def test_connect_creates_directory_and_parent(tmp_path: Path) -> None:
    """connect() 应自动创建父目录、设 0700 权限，并创建 LanceDB 数据根目录。"""
    target = tmp_path / "nested" / "sub" / "lancedb"
    mgr = LanceDBManager(target)
    mgr.connect()

    # 父目录存在且权限不低于 0700（含 owner rwx；umask 下允许低比特 0）
    assert target.parent.is_dir()
    parent_mode = stat.S_IMODE(os.stat(target.parent).st_mode)
    assert (parent_mode & 0o700) == 0o700, f"父目录权限 {oct(parent_mode)} 不含 owner rwx"
    # LanceDB 0.37 在空库场景下可能延迟写盘（先建根目录、内容等首次 add 才落盘）。
    # 用"至少根目录存在 + 是目录"作为 connect 成功断言。
    assert target.exists() and target.is_dir()

    # 再执行一次 ensure_table 强制触发第一次真实落盘，验证确实可写
    mgr.ensure_table("smoke", version=1, dim=2)
    # 落盘后至少应该看到 lance 内部文件/子目录（_versions、data.lance 等）
    after = list(target.iterdir())
    assert len(after) >= 1, "首条写入后 LanceDB 根目录下仍无文件"


def test_table_name_normalization() -> None:
    """模型名规范化：非白名单字符替换为 _，version >= 1。"""
    assert LanceDBManager.table_name("bge-large-zh-v1.5", 1) == "documents_bge-large-zh-v1.5_v1"
    # 中文/问号等非法字符 → 全变 _
    assert LanceDBManager.table_name("中文模型???", 2).startswith("documents_")
    assert LanceDBManager.table_name("中文模型???", 2).endswith("_v2")
    # version<1 抛 ValueError
    with pytest.raises(ValueError, match="version 必须"):
        LanceDBManager.table_name("bge-large-zh-v1.5", 0)


def test_ensure_table_schema_5_fields(ldb: LanceDBManager) -> None:
    """ensure_table 应创建表，schema 为 5 字段（vector/chunk_id/file_path/chunk_text/page）。"""
    tname = ldb.ensure_table("bge-large-zh-v1.5", version=1, dim=8)
    assert tname == "documents_bge-large-zh-v1.5_v1"
    assert ldb.is_table_exists(tname)

    tbl = ldb.open_table(tname)
    # schema anchor 已被删除，count_rows 应为 0（LanceDB 0.37 暴露 count_rows API）
    try:
        row_count = tbl.count_rows()
    except AttributeError:
        # 某些版本用 len(tbl) 或 pyarrow table len
        row_count = len(tbl)
    assert row_count == 0, "schema anchor 行应被删除；否则索引第一条 real chunk 后会混入假行"

    # 再写一条实数据：验证 5 列 schema 完整
    payload = [
        {
            "vector": [0.1] * 8,
            "chunk_id": "c1",
            "file_path": "/tmp/x.md",
            "chunk_text": "hello world",
            "page": 3,
        }
    ]
    tbl.add(payload)

    try:
        table_df = tbl.to_pandas()
    except Exception:  # noqa: BLE001 - pandas 未装时，用 PyArrow 取列
        df = tbl.to_arrow()
    else:
        df = table_df

    # 5 列存在
    cols = list(df.column_names) if hasattr(df, "column_names") else list(df.columns)
    for col in ("vector", "chunk_id", "file_path", "chunk_text", "page"):
        assert col in cols, f"缺列: {col}"
    # vector 维度正确（pandas→list[float] / pyarrow→list 都是 list of float）
    vec_col = df[df.column_names.index("vector")] if hasattr(df, "column_names") else df["vector"]
    row_vec = vec_col[0].as_py() if hasattr(vec_col[0], "as_py") else vec_col.iloc[0]
    assert len(row_vec) == 8
    # page=3 写入正确
    page_col = df[df.column_names.index("page")] if hasattr(df, "column_names") else df["page"]
    page_val = page_col[0].as_py() if hasattr(page_col[0], "as_py") else int(page_col.iloc[0])
    assert int(page_val) == 3


def test_ensure_table_idempotent(ldb: LanceDBManager) -> None:
    """连续两次 ensure_table 不抛错；表内容保持为空。"""
    t1 = ldb.ensure_table("bge-small-en", version=1, dim=4)
    t2 = ldb.ensure_table("bge-small-en", version=1, dim=4)
    assert t1 == t2
    assert ldb.is_table_exists(t1)

    tbl = ldb.open_table(t1)
    try:
        row_count = tbl.count_rows()
    except AttributeError:
        row_count = len(tbl)
    assert row_count == 0


def test_list_document_tables_filters_only_documents(ldb: LanceDBManager) -> None:
    """list_document_tables 只返回 documents_ 前缀的表。"""
    ldb.ensure_table("m_a", version=1, dim=2)
    ldb.ensure_table("m_b", version=1, dim=2)
    ldb.ensure_table("m_b", version=2, dim=2)

    names = sorted(ldb.list_document_tables())
    assert names == [
        "documents_m_a_v1",
        "documents_m_b_v1",
        "documents_m_b_v2",
    ]
    # list_tables 可能包含 lance 内部元表（_transactions/_versions 等），
    # 但 list_document_tables 必须只返回 documents_ 前缀的
    for n in names:
        assert n.startswith("documents_")
