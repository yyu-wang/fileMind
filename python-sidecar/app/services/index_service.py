"""增量索引判断逻辑：content_hash + embedding_version 双重判断。

API 规格书（``04_API详细规格书.html`` 第 1054-1059 行）原文：
    对每个变更文件：
    1. content_hash != stored_hash → 文件内容变了 → 重新 embedding
    2. embedding_version != stored_embedding_version → 模型变了 → 重新 embedding
    3. 两者都没变 → 跳过（即使文件 mtime 变了）

本模块是纯函数，不依赖 LanceDB / SQLite；调用方负责从 DB 读取 stored_hash
和 stored_embedding_version，传入 changes 数组，拿到 indexed/skipped/deleted
列表后自行执行实际的 embedding / LanceDB 删除。
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field

# 以下 field 描述使用 object → 需关闭 explicit-any
# pyproject.toml 中 [[tool.mypy.overrides]] module = "app.services.index_service"


class IncrementalChange(BaseModel):
    """单个文件的变更描述（由调用方从 fs-watch 或扫描结果构造）。"""

    file_id: str = Field(description="文件唯一标识（与 SQLite files.id 对应）")
    file_path: str = Field(description="文件绝对路径")
    change_type: Literal["created", "modified", "deleted"] = Field(
        description="变更类型：created=新增 / modified=修改 / deleted=删除",
    )
    content_hash: str = Field(description="新计算的文件内容 hash（SHA-256 hex）")
    stored_hash: str | None = Field(
        default=None,
        description="数据库中已存储的 hash；None 表示新文件（从未索引过）",
    )
    stored_embedding_version: int | None = Field(
        default=None,
        description="已存储向量的 Embedding 版本；None 表示从未 embedding 过",
    )


class IncrementalResult(BaseModel):
    """增量判断结果：按 file_id 分组的三类列表。"""

    indexed: list[str] = Field(
        default_factory=list,
        description="需要重新 embedding 的 file_id 列表（内容或模型版本变了）",
    )
    skipped: list[str] = Field(
        default_factory=list,
        description="跳过的 file_id 列表（hash 和 embedding 版本都没变）",
    )
    deleted: list[str] = Field(
        default_factory=list,
        description="需要从 LanceDB 删除向量的 file_id 列表（文件已删除）",
    )


def evaluate_incremental(
    changes: list[IncrementalChange],
    current_embedding_version: int,
) -> IncrementalResult:
    """对一批变更文件执行双重判断，返回分类结果。

    判断规则（API 规格书 §POST /index/incremental）：
        - ``change_type == "deleted"`` → deleted 列表（从 LanceDB 移除向量）
        - ``stored_hash is None`` → indexed（新文件，从未索引过）
        - ``content_hash != stored_hash`` → indexed（文件内容变了）
        - ``stored_embedding_version != current_embedding_version`` → indexed（模型版本变了）
        - 以上都不满足 → skipped（hash 和版本都没变，跳过）

    Args:
        changes: 变更文件列表
        current_embedding_version: 当前使用的 Embedding 模型版本

    Returns:
        :class:`IncrementalResult`：indexed / skipped / deleted 三个 file_id 列表
    """
    result = IncrementalResult()

    for change in changes:
        # 规则 0：删除 → 直接进 deleted 列表
        if change.change_type == "deleted":
            result.deleted.append(change.file_id)
            continue

        # 规则 1：新文件（stored_hash 为 None）→ 需要索引
        if change.stored_hash is None:
            result.indexed.append(change.file_id)
            continue

        # 规则 2：内容变了（content_hash != stored_hash）→ 需要重新索引
        if change.content_hash != change.stored_hash:
            result.indexed.append(change.file_id)
            continue

        # 规则 3：embedding 版本变了 → 需要重新索引
        if (
            change.stored_embedding_version is not None
            and change.stored_embedding_version != current_embedding_version
        ):
            result.indexed.append(change.file_id)
            continue

        # 规则 4：hash 和版本都没变 → 跳过（即使 mtime 变了）
        result.skipped.append(change.file_id)

    return result
