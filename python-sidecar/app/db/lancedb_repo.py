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
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable
    from pathlib import Path

    # 惰性导入（P2-1）：lancedb 冷导入约 0.7s（连带 lance_namespace / pyarrow 扩展），
    # 仅在真正建立连接时加载，避免拖慢 Sidecar 启动（/health 可服务时间）。
    # 运行时导入见 ``LanceDBManager.connect``。
    import lancedb
    from lancedb.table import Table as LanceTable

# LanceDB 0.37.1 使用 pa.Table 创建 schema；这里通过 Pydantic 兼容层描述
from pydantic import BaseModel, Field

# 合法模型名的字符：字母数字 - _ . → 非白名单字符一律替换为 _
_MODEL_NAME_FORBIDDEN_RE = re.compile(r"[^A-Za-z0-9._-]")


def _suffix_is_digits(chunk_id: str, file_id: str) -> bool:
    """chunk_id 去掉 ``{file_id}-`` 前缀后是否为纯数字（SC-C3 精确匹配辅助）。"""
    suffix = chunk_id[len(file_id) + 1 :]
    return suffix.isdigit()


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
    # 惰性求值：``from __future__ import annotations`` 下此为字符串注解，
    # 不在导入期解析 lancedb.DBConnection（该导入在 TYPE_CHECKING 分支）
    _db: lancedb.DBConnection | None = None
    #: 表句柄缓存：open_table 命中后跳过列目录/读元数据（T10.2 检索首 token 优化）。
    #: 表被删除重建时须调 invalidate_table/invalidate_all 使旧句柄失效。
    _table_cache: dict[str, LanceTable] = field(default_factory=dict, init=False)

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
        # P2-1：惰性导入（模块级不导入 lancedb，见文件头 TYPE_CHECKING 说明）
        import lancedb

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

    def delete_chunks_by_file_id(self, table_name: str, file_id: str) -> int:
        """删除指定文件的全部向量行（chunk_id = ``{file_id}-{seq}`` 精确匹配）。

        SC-C3：``build_index`` 重复写入前的旧数据清理。匹配规则用
        ``regexp_match(chunk_id, '^{file_id}-[0-9]+$')``——比
        ``LIKE '{file_id}-%'`` 多挡住「file_id 互为前缀」的误删
        （如 ``abc`` 与 ``abc-1``：LIKE 会把 ``abc-1-0`` 也算进 ``abc``）。
        ``file_id`` 先经白名单校验（仅字母/数字/连字符，无正则元字符，
        防注入）；表不存在时静默返回 0（首次 build 无旧行可删）。

        Args:
            table_name: 向量表名。
            file_id: 文件 ID（须过白名单，否则拒绝删除）。

        Returns:
            删除的行数（表不存在返回 0）。
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        # 白名单：字母/数字/连字符之外全部拒绝（防正则注入 + 误匹配）
        if not file_id or not all(ch.isalnum() or ch == "-" for ch in file_id):
            raise ValueError(f"非法 file_id: {file_id!r}")
        if not self.is_table_exists(table_name):
            return 0
        tbl = self.open_table(table_name)
        pattern = f"^{file_id}-[0-9]+$"
        try:
            deleted = tbl.delete(where=f"regexp_match(chunk_id, '{pattern}')")
        except Exception:  # noqa: BLE001 - 旧版 lancedb 不支持 regexp_match 谓词
            # 兜底两步法：LIKE 前缀取回 → Python 端按余部纯数字精确过滤。
            # 取回时只查 chunk_id 一列，避免大字段拷贝。
            rows = (
                tbl.search()
                .select(["chunk_id"])
                .where(f"chunk_id LIKE '{file_id}-%'")
                .limit(100_000)
                .to_list()
            )
            exact = [
                str(r["chunk_id"]) for r in rows if _suffix_is_digits(str(r["chunk_id"]), file_id)
            ]
            if not exact:
                return 0
            quoted = ",".join(f"'{c}'" for c in exact)
            tbl.delete(where=f"chunk_id IN ({quoted})")
            return len(exact)
        # 新版 lancedb delete 返回 DeleteResult(num_deleted_rows=...)；
        # 旧版返回 None → 保守 -1 表示「已执行删除（条数未知）」
        # DeleteResult 的 num_deleted_rows 在当前类型 stub 中缺失（运行时存在），
        # 用 getattr 访问以通过 mypy；取不到时保守 -1（条数未知）
        if deleted is None:
            return -1
        return int(getattr(deleted, "num_deleted_rows", -1))

    def add_chunks(self, table_name: str, chunks: Iterable[DocumentChunk]) -> int:
        """批量写入文档分块向量。

        追加写入（LanceDB add 语义）；调用方保证 chunk_id 不重复（建议
        ``{file_id}-{seq}`` 或先清理旧 file_id 行）。返回写入条数。

        Args:
            table_name: 向量表名（``documents_{model}_v{version}``）。
            chunks: 待写入的分块（vector 须已填充）。

        Raises:
            KeyError: 表不存在。
            RuntimeError: 连接未初始化。
        """
        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        if not self.is_table_exists(table_name):
            raise KeyError(f"LanceDB 表不存在: {table_name}")
        rows = [c.model_dump() for c in chunks]
        if not rows:
            return 0
        tbl = self.open_table(table_name)
        tbl.add(rows)
        return len(rows)

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
        # P2-1：numpy 惰性导入（模块级不导入，避免启动期加载）
        import numpy as np

        if self._db is None:
            raise RuntimeError("LanceDBManager.connect() 尚未调用")
        # 表不存在（尚未建索引）→ 返回空列表，混合检索退化为纯 FTS 排序。
        # 走 try-except 而非显式 is_table_exists：命中句柄缓存时零元数据开销
        # （T10.2）；open_table 内部仅在冷启动时列一次目录。
        try:
            tbl = self.open_table(table_name)
        except KeyError:
            return []
        # SC-m13：过滤 __schema_anchor__ 残留行（建表锚点崩溃未删时混入 top-k）
        # SC-m14：查询向量 L2 归一化——bge 模型官方推荐 cosine，LanceDB 默认 L2；
        # 归一化后 L2 排序等价于 cosine 排序（bge 输出本身已归一化，此为防御非默认模型）
        norm_vec = np.asarray(query_vector, dtype=np.float32)
        norm = np.linalg.norm(norm_vec)
        if norm > 0:
            norm_vec = norm_vec / norm
        rows = (
            tbl.search(norm_vec.tolist())
            .where("chunk_id != '__schema_anchor__'")
            .limit(top_k)
            .to_list()
        )
        # SC-m25：LanceDB ANN 对距离相同的向量不保证稳定序（底层 HNSW/IVF
        # 按插入顺序返回等距候选）。显式二次排序 → distance 升序 → chunk_id
        # 升序，使向量检索输出全确定性，跨查询 / 跨模式结果一致。
        hits = [
            VectorHit(
                chunk_id=str(row["chunk_id"]),
                file_path=str(row["file_path"]),
                chunk_text=str(row["chunk_text"]),
                page=int(row["page"]),
                distance=float(row["_distance"]),
            )
            for row in rows
        ]
        hits.sort(key=lambda h: (h.distance, h.chunk_id))
        return hits
