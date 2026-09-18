"""索引建立与增量同步（/index/*）相关请求与响应模型。

原 ``app/models/__init__.py``（374 行）拆出，由该包 ``__init__`` 再导出，
``from app.models import IndexBuildRequest`` 等调用方路径不变。
"""

from __future__ import annotations

from pydantic import BaseModel, Field

from app.services.index_service import IncrementalChange  # noqa: TC001


class IndexBuildResponse(BaseModel):
    indexed_count: int
    skipped_count: int
    indexed_file_ids: list[str] = Field(
        default_factory=list,
        description="实际写入向量的 file_id（供 Rust 回写索引状态标记）",
    )


class IndexBuildFile(BaseModel):
    """单个待索引文件（来自 Rust SQLite files 表）。"""

    file_id: str
    path: str


class IndexBuildRequest(BaseModel):
    """POST /index/build 请求体（T7.x 建立索引）。"""

    files: list[IndexBuildFile]
    embedding_model: str = "bge-large-zh-v1.5"
    table_name: str = ""


class IncrementalIndexResponse(BaseModel):
    """增量索引响应（对齐 API 规格书 POST /index/incremental）。"""

    indexed: int = 0
    skipped: int = 0
    deleted: int = 0
    duration_ms: int = 0


class IncrementalChangeRequest(BaseModel):
    """增量索引请求体（对齐 API 规格书 POST /index/incremental）。"""

    changes: list[IncrementalChange]
    embedding_model: str
    embedding_version: int
    table_name: str


class IndexPathUpdateItem(BaseModel):
    """单个文件的最新路径（分类移动/撤销后同步向量索引用）。"""

    file_id: str
    path: str


class IndexPathUpdateRequest(BaseModel):
    """POST /index/update_paths 请求体：原地更新向量行 file_path（不重新 embedding）。"""

    table_name: str
    mappings: list[IndexPathUpdateItem]


class IndexPathUpdateResponse(BaseModel):
    """POST /index/update_paths 响应体。"""

    updated: int


class IndexDeleteByFileIdsRequest(BaseModel):
    """POST /index/delete_by_file_ids 请求体：从向量索引删除指定文件的全部向量行。"""

    table_name: str
    file_ids: list[str]


class IndexDeleteByFileIdsResponse(BaseModel):
    """POST /index/delete_by_file_ids 响应体。"""

    deleted_files: int
