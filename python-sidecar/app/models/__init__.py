from __future__ import annotations

from pydantic import BaseModel

from app.services.index_service import IncrementalChange  # noqa: TC001


class HealthResponse(BaseModel):
    status: str
    version: str
    uptime_seconds: float


class ClassifyResponse(BaseModel):
    items: list[dict[str, object]]
    stats: dict[str, object]
    confidence: float


class IndexBuildResponse(BaseModel):
    indexed_count: int
    skipped_count: int


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


class EmbeddingModelsResponse(BaseModel):
    models: list[str]


class EmbeddingSwitchResponse(BaseModel):
    status: str
    model: str


class ChatQueryResponse(BaseModel):
    answer: str
    citations: list[dict[str, object]]
    tokens: int


class ChatStreamResponse(BaseModel):
    status: str


class HandshakeChallenge(BaseModel):
    """握手请求体：Rust 客户端发送的 nonce（hex 编码 64 字符）。"""

    nonce: str


class HandshakeResponse(BaseModel):
    """握手响应体：Sidecar 用 PSK 对 ``handshake-ok|{nonce}`` 签名后返回的 proof。"""

    proof: str


class ShutdownResponse(BaseModel):
    """优雅关闭响应体：POST /shutdown 成功后返回。"""

    status: str


class MetricsResponse(BaseModel):
    """Sidecar 进程内存监控响应：Go/No-Go 第 7 项（<300MB）的判定依据。"""

    rss_mb: float
    vms_mb: float
    threshold_mb: int
    within_limit: bool


class SearchRequest(BaseModel):
    """搜索请求体：POST /search。

    Sidecar 仅做"查询构造"——jieba 分词 + 转义拼装，不直接访问 SQLite
    （项目硬约束：所有 DB 操作由 Rust 层执行）。Rust 层拿到 fts_query 后
    通过 rusqlite 执行 MATCH。
    """

    query: str
    top_k: int = 20
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
