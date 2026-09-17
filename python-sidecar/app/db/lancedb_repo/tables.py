"""表名规范与表生命周期（原 ``lancedb_repo.py`` 拆出，混入 ``LanceDBManager``）。

表名规范：``documents_{embedding_model}_v{version}``
（模型名中非法字符做安全替换，参考 :meth:`TableOpsMixin.table_name`）。
"""

from __future__ import annotations

import re
from typing import TYPE_CHECKING

from app.db.lancedb_repo.types import DocumentChunk

if TYPE_CHECKING:
    from collections.abc import Iterable
    from pathlib import Path

    # 惰性导入（P2-1）：lancedb 冷导入约 0.7s（连带 lance_namespace / pyarrow 扩展），
    # 仅在真正建立连接时加载，避免拖慢 Sidecar 启动（/health 可服务时间）。
    # 运行时导入见 ``LanceDBManager.connect``。
    import lancedb
    from lancedb.table import Table as LanceTable

# 合法模型名的字符：字母数字 - _ . → 非白名单字符一律替换为 _
_MODEL_NAME_FORBIDDEN_RE = re.compile(r"[^A-Za-z0-9._-]")


class TableOpsMixin:
    """规范表名与表的建/开/查/缓存失效（由 ``LanceDBManager`` 混入）。

    属性声明仅为满足 mypy strict：真实字段定义在 ``LanceDBManager``（dataclass）。
    """

    if TYPE_CHECKING:
        db_path: Path
        _db: lancedb.DBConnection | None
        _table_cache: dict[str, LanceTable]

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
        """确保指定模型版本的表存在（规范化表名）。

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
        tname = self.table_name(model, version)
        return self.ensure_table_named(tname, dim)

    def ensure_table_named(self, table_name: str, dim: int) -> str:
        """确保指定名称的向量表存在（幂等，不做名称规范化）。

        与 :meth:`ensure_table` 的区别：直接按调用方给出的表名建表，
        不套用 ``documents_{model}_v{version}`` 规范——供基准隔离表
        （``..._bench``）等非规范表名使用；规范名场景两者等价。

        Args:
            table_name: 目标表名（原样使用）。
            dim: 向量维度（建表时写入一条长度为 dim 的零向量作为 schema 锚点）。

        Returns:
            建表/已存在的表名（等于 ``table_name``）。

        Raises:
            ValueError: dim < 1 或 table_name 为空。
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        if dim < 1:
            raise ValueError(f"dim 必须 >= 1，当前={dim}")
        if not table_name:
            raise ValueError("table_name 不能为空")
        tname = table_name

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
        # 建表可能覆盖同名旧表（外部重建场景），丢弃可能存在的过期句柄
        self.invalidate_table(tname)
        return tname

    def open_table(self, table_name: str) -> LanceTable:
        """打开已存在的表，命中句柄缓存则直接返回（LanceDB 表操作每次读最新版本）。

        表句柄在 LanceDB 中可跨版本复用，缓存避免每次检索重复列目录/读元数据。
        表被删除重建时调用 :meth:`invalidate_table` 使旧句柄失效。
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        cached = self._table_cache.get(table_name)
        if cached is not None:
            return cached
        if not self.is_table_exists(table_name):
            raise KeyError(f"LanceDB 表不存在: {table_name}")
        tbl = self._db.open_table(table_name)
        self._table_cache[table_name] = tbl
        return tbl

    def invalidate_table(self, table_name: str) -> None:
        """丢弃某表的句柄缓存（表被删除/重建后调用，避免操作失效句柄）。"""
        self._table_cache.pop(table_name, None)

    def invalidate_all(self) -> None:
        """丢弃全部表句柄缓存（整体重建索引时调用）。"""
        self._table_cache.clear()
