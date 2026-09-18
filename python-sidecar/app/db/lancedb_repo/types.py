"""LanceDB 表的行模型（原 ``lancedb_repo.py`` 拆出）。

Schema 来源：`02_总体实施计划.html §4.2`：
    vector      FLOAT[dim]
    chunk_id    TEXT
    file_path   TEXT
    chunk_text  TEXT
    page        INTEGER (默认 0)
"""

from __future__ import annotations

from dataclasses import dataclass

from pydantic import BaseModel, Field


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
