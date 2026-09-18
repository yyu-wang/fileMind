"""问答与检索（/chat、/search）相关请求与响应模型。

原 ``app/models/__init__.py``（374 行）拆出，由该包 ``__init__`` 再导出，
``from app.models import ChatStreamRequest`` 等调用方路径不变。
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class ChatQueryResponse(BaseModel):
    answer: str
    citations: list[dict[str, object]]
    tokens: int


class ChatTurn(BaseModel):
    """一轮对话历史（P-02 查询改写输入变量 conversation_history）。"""

    user: str
    assistant: str


class ChatChunkInput(BaseModel):
    """FTS5 命中（Rust 层从 SQLite 提供，含原文文本）。

    Sidecar 永不碰 SQLite（硬约束），FTS 结果由 Rust 执行后随请求传入；
    ``text`` 用于 FTS-only 命中补全 P-03 生成上下文（向量命中文本在 LanceDB）。
    """

    chunk_id: str
    text: str
    file_path: str = ""
    page: int = 0


class ChatStreamRequest(BaseModel):
    """POST /chat/stream 请求体（04_API详细规格书 §3.4 + 分层演进扩展）。

    扩展说明（sidecar/Rust 分层下 API 规格书请求体的必然演进）：
      - ``history``：P-02 查询改写需要对话历史（API 规格书仅有 session_id，
        Rust 维护会话状态并回传最近 3 轮）
      - ``fts_chunks``：FTS5 由 Rust 执行（SQLite），命中含文本随请求传入
      - ``llm_model`` / ``embedding_model``：生成/向量化模型名（逐请求可覆盖）
      - ``max_retries``：P-04 自我纠正最大重试次数（默认 2，对齐 §3.4 请求体）
      - ``inference_mode``：local|cloud|hybrid；本任务仅 local（云端代理 T6.x）
    """

    query: str
    history: list[ChatTurn] = Field(default_factory=list)
    table_name: str
    embedding_model: str = "bge-large-zh-v1.5"
    inference_mode: str = "local"
    llm_model: str = "qwen3.8-27b"
    # SC-m15：加 ge=1 约束——客户端传 0 会导致 FTS LIMIT 0 返回空
    top_k: int = Field(default=20, ge=1)
    rerank_top_k: int = Field(default=5, ge=1)
    max_retries: int = Field(default=2, ge=1)
    fts_chunks: list[ChatChunkInput] = Field(default_factory=list)
    session_id: str | None = None


class SearchRequest(BaseModel):
    """搜索请求体：POST /search。

    Sidecar 仅做"查询构造"——jieba 分词 + 转义拼装，不直接访问 SQLite
    （项目硬约束：所有 DB 操作由 Rust 层执行）。Rust 层拿到 fts_query 后
    通过 rusqlite 执行 MATCH。
    """

    query: str
    # SC-m15：加 ge=1 约束
    top_k: int = Field(default=20, ge=1)
    mode: str = "fts"  # fts | vector | hybrid


class SearchResponse(BaseModel):
    """搜索响应体：返回构造好的 FTS5 MATCH 表达式 + 分词结果。

    - ``fts_query``：FTS5 MATCH 表达式，每个 token 已用双引号包裹防语法注入
    - ``tokens``：jieba 分词结果，便于前端高亮命中词
    - ``top_k`` / ``mode``：调用方传入的执行参数，原样回显
    """

    fts_query: str
    tokens: list[str]
    top_k: int
    mode: str
