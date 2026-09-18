"""向量 ANN 检索（原 ``lancedb_repo.py`` 拆出，混入 ``LanceDBManager``）。"""

from __future__ import annotations

from app.db.lancedb_repo.tables import TableOpsMixin
from app.db.lancedb_repo.types import VectorHit


class VectorSearchMixin(TableOpsMixin):
    """向量检索（由 ``LanceDBManager`` 混入）。"""

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
