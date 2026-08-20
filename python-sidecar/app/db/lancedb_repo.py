"""LanceDB 连接管理与 schema 定义。

表名规范：``documents_{embedding_model}_v{version}``
（模型名中非法字符做安全替换，参考 ``_sanitize_model_name``。）

Schema 来源：`02_总体实施计划.html §4.2`：
    vector      FLOAT[dim]
    chunk_id    TEXT
    file_path   TEXT
    chunk_text  TEXT
    page        INTEGER (默认 0)

安全约束（`07_安全合规设计.html` 威胁分析）：
    - 存储目录 ~/.filemind/data/lancedb 权限 0600
    - 模型版本切换 = 建新表，不原地修改旧表（旧表保留便于回滚）
"""

from __future__ import annotations

import contextlib
import os
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

import lancedb

if TYPE_CHECKING:
    from collections.abc import Iterable
    from pathlib import Path

    from lancedb.table import Table as LanceTable

# LanceDB 0.37.1 使用 pa.Table 创建 schema；这里通过 Pydantic 兼容层描述
from pydantic import BaseModel, Field

# 合法模型名的字符：字母数字 - _ . → 非白名单字符一律替换为 _
_MODEL_NAME_FORBIDDEN_RE = re.compile(r"[^A-Za-z0-9._-]")


class DocumentChunk(BaseModel):
    """LanceDB documents_* 表的行 schema。"""

    vector: list[float] = Field(description="Embedding 向量，维度 dim 与模型绑定")
    chunk_id: str = Field(description="与 SQLite FTS5 行关联的 chunk 唯一标识")
    file_path: str = Field(description="源文件绝对路径")
    chunk_text: str = Field(description="原文片段，用于 RAG 检索时展示与重排")
    page: int = Field(default=0, ge=0, description="PDF/Word 页码（可选，默认 0）")


@dataclass(frozen=True)
class VectorHit:
    """一次向量检索的命中行（DocumentChunk 字段 + 检索距离）。"""

    chunk_id: str
    file_path: str
    chunk_text: str
    page: int
    distance: float  # LanceDB _distance（默认 L2，越小越近）


@dataclass
class LanceDBManager:
    """LanceDB 生命周期管理器（FastAPI lifespan 里单例初始化）。"""

    db_path: Path
    _db: lancedb.DBConnection | None = None  # noqa: F821 - lancedb 动态属性

    # ------------------------------------------------------------------
    # 连接 / 初始化
    # ------------------------------------------------------------------

    def connect(self) -> None:
        """创建/打开 LanceDB。

        行为：
            1. 确保父目录存在，权限 0700（owner rwx 仅自己）
            2. 目录不存在时 mkdir(parents=True)
            3. lancedb.connect(self.db_path)
            4. 对生成的 LanceDB 数据文件/目录设 0600（owner rw only）

        Raises:
            RuntimeError: LanceDB 连接失败时，抛带路径上下文的异常
        """
        try:
            self.db_path.parent.mkdir(parents=True, exist_ok=True)
            # 父目录 0700：仅 owner 可进入/读/写
            os.chmod(self.db_path.parent, 0o700)
            self._db = lancedb.connect(str(self.db_path))
            # 连接成功后，为 db_path 目录再降权：0700 即可（不阻止子文件写入）
            if self.db_path.exists():
                # 某些文件系统不支持 chmod（samba、tmpfs...），不阻塞启动
                with contextlib.suppress(OSError):
                    os.chmod(self.db_path, 0o700)
        except Exception as exc:  # noqa: BLE001
            raise RuntimeError(f"LanceDB 初始化失败（path={self.db_path}）: {exc}") from exc

    # ------------------------------------------------------------------
    # 表名规范
    # ------------------------------------------------------------------

    @staticmethod
    def table_name(model: str, version: int = 1) -> str:
        """生成规范化表名：``documents_{safe_model_name}_v{version}``。

        示例：
            table_name("bge-large-zh-v1.5", 1)
            → "documents_bge-large-zh-v1.5_v1"  （. 是合法字符，仅 _ 替换非白名单）
            table_name("中文模型???", 2)
            → "documents___v2"
        """
        if version < 1:
            raise ValueError(f"version 必须 >= 1，当前={version}")
        safe = _MODEL_NAME_FORBIDDEN_RE.sub("_", model)
        if not safe:
            raise ValueError(f"model 名规范化后为空（原始 model={model!r}）")
        return f"documents_{safe}_v{version}"

    # ------------------------------------------------------------------
    # 表操作
    # ------------------------------------------------------------------

    @staticmethod
    def _coerce_table_names(resp: object) -> list[str]:
        """把 LanceDB 各种 list_table_names() / list_tables() 返回值归一成 list[str]。

        LanceDB 0.37 的 ``DBConnection.list_tables()`` 返回的是
        ``ListTablesResponse``（Pydantic model，带 ``.tables: list[str]``）；
        老版本 ``table_names()`` 返回 ``list[str]``；还有一些版本返回
        ``tuple[str, ...]``。这里兼容所有实现。
        """
        # 优先走 ListTablesResponse.tables（新 SDK）
        tables_attr = getattr(resp, "tables", None)
        if isinstance(tables_attr, list):
            return [str(t) for t in tables_attr]
        # list[str] / tuple[str, ...]
        if isinstance(resp, (list, tuple)):
            return [str(t) for t in resp]
        return []

    def is_table_exists(self, table_name: str) -> bool:
        """检查表是否存在（新版 ``list_tables()`` / 旧版 ``table_names()`` 都兼容）。"""
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        names = self._raw_table_names()
        return table_name in names

    def list_tables(self) -> list[str]:
        """列出所有 LanceDB 表名（包含非 documents_ 前缀的表，供调试用）。"""
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        return self._raw_table_names()

    def _raw_table_names(self) -> list[str]:
        """内部：``list_tables()`` 可用就用新版，否则退回 ``table_names()``。"""
        assert self._db is not None
        if hasattr(self._db, "list_tables"):
            return self._coerce_table_names(self._db.list_tables())
        legacy = getattr(self._db, "table_names", None)
        if callable(legacy):
            return self._coerce_table_names(legacy())
        return []

    def list_document_tables(self) -> list[str]:
        """列出所有 documents_ 开头的表（即 Embedding 模型对应的向量表）。"""
        return [t for t in self.list_tables() if t.startswith("documents_")]

    def ensure_table(
        self,
        model: str,
        version: int,
        dim: int,
    ) -> str:
        """确保指定模型版本的表存在。

        步骤：
            1. 计算规范化表名
            2. 若表不存在：用 DocumentChunk schema 建空表（首条写入会验证 dim）
            3. 返回最终表名

        Args:
            model: Embedding 模型名（如 bge-large-zh-v1.5）
            version: 版本号（>=1）
            dim: 向量维度（建表时写入一条长度为 dim 的零向量作为 schema 锚点；
                 LanceDB 0.37 要求 create_table 传入非空数据才能推断 schema）

        Returns:
            规范化后的表名

        Raises:
            ValueError: dim < 1 / version < 1 / 模型名非法
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        if dim < 1:
            raise ValueError(f"dim 必须 >= 1，当前={dim}")

        tname = self.table_name(model, version)
        if self.is_table_exists(tname):
            return tname

        # 用一条零向量作为 schema 锚点；生产数据写入时 LanceDB 会按列类型合并
        anchor: Iterable[dict[str, object]] = [
            DocumentChunk(
                vector=[0.0] * dim,
                chunk_id="__schema_anchor__",
                file_path="__schema_anchor__",
                chunk_text="",
                page=0,
            ).model_dump()
        ]
        self._db.create_table(tname, data=anchor)
        # 立即把锚点行删除：真实索引开始时，第一行应该是真实 chunk
        tbl = self._db.open_table(tname)
        tbl.delete("chunk_id = '__schema_anchor__'")
        return tname

    def open_table(self, table_name: str) -> LanceTable:
        """打开已存在的表（返回 LanceDB Table，调用方负责写入/检索）。"""
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        if not self.is_table_exists(table_name):
            raise KeyError(f"LanceDB 表不存在: {table_name}")
        return self._db.open_table(table_name)

    def search_vectors(
        self,
        table_name: str,
        query_vector: list[float],
        top_k: int = 20,
    ) -> list[VectorHit]:
        """对向量表执行 ANN 检索，返回按距离升序的命中列表。

        表不存在（尚未建索引）→ 返回空列表，混合检索退化为纯 FTS 排序。
        检索行包含 DocumentChunk 字段 + ``_distance``（默认 L2 距离，越小越近）。

        Args:
            table_name: 向量表名（``documents_{model}_v{version}``）。
            query_vector: 查询向量（维度须与表 schema 一致）。
            top_k: 截断候选数（默认 20，对齐 RAG 流水线「向量 Top-20」）。

        Returns:
            按 ``_distance`` 升序的命中列表；表不存在时返回空列表。
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        if not self.is_table_exists(table_name):
            return []
        tbl = self.open_table(table_name)
        rows = tbl.search(query_vector).limit(top_k).to_list()
        return [
            VectorHit(
                chunk_id=str(row["chunk_id"]),
                file_path=str(row["file_path"]),
                chunk_text=str(row["chunk_text"]),
                page=int(row["page"]),
                distance=float(row["_distance"]),
            )
            for row in rows
        ]
