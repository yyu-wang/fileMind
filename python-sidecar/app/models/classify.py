"""分类（/classify）相关请求与响应模型。

原 ``app/models/__init__.py``（374 行）拆出，由该包 ``__init__`` 再导出，
``from app.models import ClassifyItem`` 等调用方路径不变。
"""

from __future__ import annotations

from pydantic import BaseModel


class ClassifyResponse(BaseModel):
    items: list[dict[str, object]]
    stats: dict[str, object]
    confidence: float


class ClassifyItem(BaseModel):
    """单个待分类文件（对齐 04_API详细规格书 §3.2 files 元素 + P-01 输入变量）。

    ``content_summary`` / ``modified_time`` 由调用方（Rust 层扫描后）提供，
    Sidecar 不做文件 I/O。``path`` 为文件绝对路径，用于目录信号与 LLM 上下文。
    """

    name: str
    extension: str = ""
    path: str
    size: int = 0
    content_summary: str = ""
    modified_time: str = ""


class ClassifyRequest(BaseModel):
    """POST /classify 请求体。

    ``categories`` 为 SQLite categories 表预定义分类列表（P-01 输入变量
    ``predefined_categories``），由 Rust 层传入；所有 DB 操作在 Rust 侧。
    ``llm_model`` 指定生成模型名（T8.5 云端适配）：空串回落本地默认
    ``LLM_MODEL``，云端前缀（gpt-* / deepseek-*）走云端推理。
    """

    files: list[ClassifyItem]
    categories: list[str] = []
    llm_model: str = ""
