"""Sidecar 的 Pydantic 请求/响应模型集合。

模块划分（原单文件 374 行，逼近 Python 模块 500 行强制阈值，见 `rules/complexity.md`）：
按接口域拆为 ``classify`` / ``chat`` / ``index`` / ``embedding`` / ``system`` 五个子模块，
本包同名保留原导入路径——``from app.models import ChatStreamRequest`` 等 30+ 处调用方零改动。

``_OllamaTag*``（Ollama ``/api/tags`` 的内部映射结构）**不在**本出口：它们是
``services/inference_probe_service`` 的实现细节，由该模块直接从 ``app.models.system`` 导入，
避免私有名经包级出口造成隐式再导出。
"""

from app.models.chat import (
    ChatChunkInput,
    ChatQueryResponse,
    ChatStreamRequest,
    ChatTurn,
    SearchRequest,
    SearchResponse,
)
from app.models.classify import ClassifyItem, ClassifyRequest, ClassifyResponse
from app.models.embedding import (
    EmbeddingModelAvailability,
    EmbeddingModelsResponse,
    EmbeddingSwitchRequest,
    EmbeddingSwitchResponse,
    EmbeddingSwitchResult,
    ModelDownloadRequest,
    ModelDownloadStatusResponse,
    ModelImportRequest,
    ModelImportResponse,
    RebuildStatusResponse,
)
from app.models.index import (
    IncrementalChangeRequest,
    IncrementalIndexResponse,
    IndexBuildFile,
    IndexBuildRequest,
    IndexBuildResponse,
    IndexDeleteByFileIdsRequest,
    IndexDeleteByFileIdsResponse,
    IndexPathUpdateItem,
    IndexPathUpdateRequest,
    IndexPathUpdateResponse,
)
from app.models.system import (
    HandshakeChallenge,
    HandshakeResponse,
    HealthResponse,
    InferenceTestResponse,
    MetricsResponse,
    OllamaModelInfo,
    ShutdownResponse,
)

__all__ = [
    "ChatChunkInput",
    "ChatQueryResponse",
    "ChatStreamRequest",
    "ChatTurn",
    "ClassifyItem",
    "ClassifyRequest",
    "ClassifyResponse",
    "EmbeddingModelAvailability",
    "EmbeddingModelsResponse",
    "EmbeddingSwitchRequest",
    "EmbeddingSwitchResponse",
    "EmbeddingSwitchResult",
    "HandshakeChallenge",
    "HandshakeResponse",
    "HealthResponse",
    "IncrementalChangeRequest",
    "IncrementalIndexResponse",
    "IndexBuildFile",
    "IndexBuildRequest",
    "IndexBuildResponse",
    "IndexDeleteByFileIdsRequest",
    "IndexDeleteByFileIdsResponse",
    "IndexPathUpdateItem",
    "IndexPathUpdateRequest",
    "IndexPathUpdateResponse",
    "InferenceTestResponse",
    "MetricsResponse",
    "ModelDownloadRequest",
    "ModelDownloadStatusResponse",
    "ModelImportRequest",
    "ModelImportResponse",
    "OllamaModelInfo",
    "RebuildStatusResponse",
    "SearchRequest",
    "SearchResponse",
    "ShutdownResponse",
]
