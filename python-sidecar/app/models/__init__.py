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
