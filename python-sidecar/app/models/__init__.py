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


class IndexBuildResponse(BaseModel):
    indexed_count: int
    skipped_count: int


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


class EmbeddingModelsResponse(BaseModel):
    models: list[str]


class EmbeddingSwitchResponse(BaseModel):
    status: str
    model: str


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


class OllamaModelInfo(BaseModel):
    """本地 Ollama 已安装的生成模型信息（来自 /api/tags，T6.7 探测用）。"""

    name: str
    size_bytes: int = Field(ge=0, description="模型文件大小（字节）")
    family: str | None = Field(default=None, description="模型家族（details.family，可能缺失）")
    modified_at: str | None = Field(default=None, description="最近修改时间（ISO 8601）")


class EmbeddingModelAvailability(BaseModel):
    """单个 Embedding 模型在本地 Ollama 的可用性（T6.7 探测用）。"""

    name: str
    dim: int = Field(ge=0, description="向量维度")
    version: int = Field(ge=1, description="当前分配版本号")
    available: bool = Field(description="对应 Ollama 模型是否已安装")


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


class ModelInstallRequest(BaseModel):
    """POST /inference/install-model 请求体：从 Ollama 拉取指定模型。"""

    model_name: str = Field(description="注册表模型名（如 bge-small-zh-v1.5）")


class ModelInstallResponse(BaseModel):
    """POST /inference/install-model 响应体。"""

    success: bool
    model_name: str
    ollama_name: str
    message: str = ""
