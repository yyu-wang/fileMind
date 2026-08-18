from __future__ import annotations

from pydantic import BaseModel, Field

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


class EmbeddingSwitchRequest(BaseModel):
    """POST /embedding/switch 请求体（API 规格书 §s3-6a）。

    说明：预检查阶段只做「是否需要全量重建」的判定，不修改任何状态；
    真实切换动作留在 T3.x（Rust 侧 IPC confirm_embedding_rebuild 触发）。
    """

    new_model: str = Field(description="目标模型名（需在 MODEL_REGISTRY 中存在）")
    current_model: str = Field(description="当前模型名")
    indexed_files: int = Field(
        ge=0, description="已索引文件数（用于预估 files_to_rebuild/est_minutes）"
    )


class EmbeddingSwitchResult(BaseModel):
    """POST /embedding/switch 返回值的 data 字段（API 规格书 §s3-6a）。

    业务规则（与 plans/T2.6 对齐）：
      - 模型名变了（即使 dim 相同）→ dim_changed=True、files_to_rebuild=indexed_files
        （向量空间不同，必须重 embedding）
      - 模型相同 → dim_changed=False、files_to_rebuild=0（下次增量索引覆盖）
      - old_table_preserved 固定为 True（rules/sql.md：版本变更新建表，不删旧表）
    """

    dim_changed: bool = Field(description="向量维度是否变化（或模型不同）")
    current_dim: int = Field(ge=0, description="当前模型向量维度")
    new_dim: int = Field(ge=0, description="目标模型向量维度")
    files_to_rebuild: int = Field(ge=0, description="预估需重建文件数")
    est_minutes: int = Field(ge=0, description="预估耗时（分钟，上限 999）")
    new_table_name: str = Field(description="目标 LanceDB 表名 documents_{model}_v{N+1}")
    new_version: int = Field(ge=1, description="目标模型分配到的新版本号")
    old_table_preserved: bool = Field(default=True, description="旧表是否保留（固定 True）")


class RebuildStatusResponse(BaseModel):
    """GET /embedding/rebuild/status 返回值的 data 字段（API 规格书 §s3-6b）。"""

    task_id: str
    status: str = Field(description="pending | in_progress | paused | done | failed | cancelled")
    done: int = Field(ge=0, description="已处理文件数")
    total: int = Field(ge=0, description="总文件数")
    current_file: str | None = Field(description="当前正在处理的文件名（展示用）")
    est_remaining_minutes: int | None = Field(description="预估剩余分钟（done=0 时为 None）")
    can_pause: bool = Field(description="是否可暂停（仅 in_progress）")
    can_resume: bool = Field(description="是否可恢复（paused/failed 状态）")
