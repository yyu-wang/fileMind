"""向量行的写入与按文件删除（原 ``lancedb_repo.py`` 拆出，混入 ``LanceDBManager``）。"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.db.lancedb_repo.tables import TableOpsMixin

if TYPE_CHECKING:
    from collections.abc import Iterable

    from app.db.lancedb_repo.types import DocumentChunk


def _suffix_is_digits(chunk_id: str, file_id: str) -> bool:
    """chunk_id 去掉 ``{file_id}-`` 前缀后是否为纯数字（SC-C3 精确匹配辅助）。"""
    suffix = chunk_id[len(file_id) + 1 :]
    return suffix.isdigit()


class ChunkOpsMixin(TableOpsMixin):
    """分块向量的增删（由 ``LanceDBManager`` 混入）。"""

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
