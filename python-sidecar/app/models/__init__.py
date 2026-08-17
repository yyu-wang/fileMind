from pydantic import BaseModel


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


class IndexIncrementalResponse(BaseModel):
    added: int
    updated: int
    deleted: int


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
