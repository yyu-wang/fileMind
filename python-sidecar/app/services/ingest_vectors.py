"""向量行的删除与路径同步（原 ``ingest_service.py`` 拆出）。

「按 file_id 维护**已存在**的向量行」这一类操作：写入前清旧行（避免重复 build
后 chunk 翻倍）、分类移动后同步路径（不重新 embedding）、目录级移除。全部只做
LanceDB 表操作，不涉及读取/分块/Embedding。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.core.logging import getLogger

if TYPE_CHECKING:
    from app.db.lancedb_repo import LanceDBManager

logger = getLogger("filemind.ingest")


def _is_safe_file_id(value: str) -> bool:
    """file_id 白名单校验：仅允许字母/数字/连字符。

    用于拼入 ``chunk_id LIKE '{file_id}-%'`` 谓词前的防御性检查——
    挡掉引号、空格、``%``/``_`` 通配符、``;`` 等注入/误匹配字符；
    真实 file_id（uuid 十六进制+连字符）必过。命中异常值时跳过该条。
    """
    return bool(value) and all(ch.isalnum() or ch == "-" for ch in value)


def _delete_stale_chunks(
    mgr: LanceDBManager,
    table_name: str,
    indexed_file_ids: set[str],
) -> None:
    """SC-C3：写入前按 file_id 删除旧向量行（add_chunks 是纯追加语义）。

    不清理则同文件重复 build 后 chunk 翻倍、检索重复；非法 file_id 跳过并告警。
    """
    for file_id in indexed_file_ids:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.delete_stale_skipped", file_id=file_id)
            continue
        mgr.delete_chunks_by_file_id(table_name, file_id)


def update_paths(
    table_name: str,
    mappings: list[tuple[str, str]],
    mgr: LanceDBManager,
) -> int:
    """把指定文件的最新路径同步到向量索引（chunk_id 前缀匹配，不重新 embedding）。

    分类移动/撤销后调用：SQLite 的 ``files.path`` 已是新路径，但向量行里的
    ``file_path`` 仍是旧路径（增量索引只认 created/modified/deleted，无移动语义）。
    这里按 ``chunk_id = {file_id}-{seq}`` 前缀原地更新 ``file_path``，向量不变。
    表不存在（从未建索引）或 ``mappings`` 为空时返回 0（静默跳过）。

    Args:
        table_name: 目标向量表名（``documents_{model}_v{version}``）。
        mappings: ``(file_id, 最新路径)`` 列表。
        mgr: LanceDB 管理器。

    Returns:
        成功更新路径的文件数（一个文件可对应多个分块行）。
    """
    if not mappings:
        return 0
    if not mgr.is_table_exists(table_name):
        return 0

    tbl = mgr.open_table(table_name)
    updated = 0
    for file_id, path in mappings:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.update_paths_skipped", file_id=file_id)
            continue
        # SC-m16：LIKE 前缀加边界——chunk_id 格式是 {file_id}-{seq}，
        # file_id 是 UUID（含连字符），{file_id}- 已含分隔符，
        # 但显式 ESCAPE 防御 file_id 是另一个 id 前缀的极端场景
        tbl.update(
            where=f"chunk_id = '{file_id}' OR chunk_id LIKE '{file_id}-%' ESCAPE '\\'",
            values={"file_path": path},
        )
        updated += 1
    # SC-m16：updated 计数是有效映射数（LanceDB update 不返回影响行数）
    return updated


def delete_by_file_ids(
    table_name: str,
    file_ids: list[str],
    mgr: LanceDBManager,
) -> int:
    """从向量索引中删除指定文件的全部向量行（目录级移除用）。

    逐个调用 :meth:`LanceDBManager.delete_chunks_by_file_id`，表不存在或
    ``file_ids`` 为空时返回 0（静默跳过）。非法 file_id 跳过并告警，
    不中断整体。

    Args:
        table_name: 目标向量表名（``documents_{model}_v{version}``）。
        file_ids: 待删除向量的文件 ID 列表。
        mgr: LanceDB 管理器。

    Returns:
        成功删除向量的文件数（一个文件对应多个分块行，计数按文件计）。
    """
    if not file_ids:
        return 0
    if not mgr.is_table_exists(table_name):
        return 0

    deleted = 0
    for file_id in file_ids:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.delete_by_file_ids_skipped", file_id=file_id)
            continue
        mgr.delete_chunks_by_file_id(table_name, file_id)
        deleted += 1
    return deleted
