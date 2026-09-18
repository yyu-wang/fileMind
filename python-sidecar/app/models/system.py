"""进程与探测（健康检查、握手、关闭、内存指标、Ollama 探测）相关模型。

原 ``app/models/__init__.py``（374 行）拆出，由该包 ``__init__`` 再导出。
``_OllamaTag*`` 是 Ollama ``/api/tags`` 的内部映射结构（仅
``services/inference_probe_service`` 使用），故**不在** ``__init__`` 中再导出，
由该服务直接从本模块导入——私有名不经包级出口，避免隐式再导出。
"""

from __future__ import annotations

from pydantic import BaseModel, Field

from app.models.embedding import EmbeddingModelAvailability  # noqa: TC001


class HealthResponse(BaseModel):
    status: str
    version: str
    uptime_seconds: float


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
    """Sidecar 进程内存监控响应：Go/No-Go 第 7 项（<500MB）的判定依据。"""

    rss_mb: float
    vms_mb: float
    threshold_mb: int
    within_limit: bool


class OllamaModelInfo(BaseModel):
    """本地 Ollama 已安装的生成模型信息（来自 /api/tags，T6.7 探测用）。"""

    name: str
    size_bytes: int = Field(ge=0, description="模型文件大小（字节）")
    family: str | None = Field(default=None, description="模型家族（details.family，可能缺失）")
    modified_at: str | None = Field(default=None, description="最近修改时间（ISO 8601）")


class InferenceTestResponse(BaseModel):
    """POST /inference/test 返回：本地 Ollama 推理环境探测结果。

    Ollama 不可用时仍返回 HTTP 200（探测是轻量状态查询，不应让应用 5xx），
    ``available=false`` + ``error_code='OLLAMA_UNAVAILABLE'`` 供前端展示。
    """

    available: bool
    status: str = Field(description="ok | unavailable")
    llm_models: list[OllamaModelInfo] = Field(default_factory=list)
    embedding_models: list[EmbeddingModelAvailability] = Field(default_factory=list)
    error_code: str | None = Field(default=None, description="OLLAMA_UNAVAILABLE 等错误码")
    message: str | None = Field(default=None, description="人类可读的失败原因（展示用）")


class _OllamaTagDetails(BaseModel):
    """/api/tags 单个模型的 details 字段（可缺失，仅取 family）。"""

    family: str | None = None


class _OllamaTagModel(BaseModel):
    """/api/tags 单个模型条目（对齐 Ollama HTTP API 实际字段）。"""

    name: str
    size: int = 0
    modified_at: str = ""
    details: _OllamaTagDetails | None = None


class _OllamaTagsResponse(BaseModel):
    """GET {OLLAMA_HOST}/api/tags 响应体。"""

    models: list[_OllamaTagModel] = Field(default_factory=list)
