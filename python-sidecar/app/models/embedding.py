"""Embedding 模型管理与模型包（/embedding/*、/models/*）相关请求与响应模型。

原 ``app/models/__init__.py``（374 行）拆出，由该包 ``__init__`` 再导出，
``from app.models import EmbeddingSwitchResult`` 等调用方路径不变。
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class EmbeddingModelsResponse(BaseModel):
    models: list[str]


class EmbeddingSwitchResponse(BaseModel):
    status: str
    model: str


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


class EmbeddingModelAvailability(BaseModel):
    """单个 Embedding 模型在本地 Ollama 的可用性（T6.7 探测用）。"""

    name: str
    dim: int = Field(ge=0, description="向量维度")
    version: int = Field(ge=1, description="当前分配版本号")
    available: bool = Field(description="对应 Ollama 模型是否已安装")


class ModelDownloadRequest(BaseModel):
    """POST /models/download 请求体：下载指定模型的 ONNX 权重与 tokenizer 文件。"""

    model_name: str = Field(description="注册表模型名（如 bge-large-zh-v1.5）")


class ModelDownloadStatusResponse(BaseModel):
    """模型下载状态：供设置页展示进度条与「未下载不可用」引导。"""

    model_name: str
    status: str = Field(description="idle（未开始）/ downloading / ready / failed")
    mirror: str | None = Field(default=None, description="当前使用的镜像地址；未开始为 None")
    attempt: int = Field(default=0, description="已尝试次数（含当前这次）")
    downloaded_bytes: int = Field(default=0, description="已下载字节数")
    total_bytes: int | None = Field(
        default=None, description="全部文件总字节数；无法探测时为 None（前端显示不确定进度）"
    )
    error: str | None = Field(default=None, description="失败原因（status=failed 时非空）")
    updated_at: str = Field(default="", description="状态最后更新时间（ISO 8601）")


class ModelImportRequest(BaseModel):
    """POST /models/import 请求体：导入离线模型包（内网 / 无外网部署用）。"""

    path: str = Field(
        description="离线包路径（zip 文件，或 models 目录 / 单个模型目录）；Rust 侧已过路径安全校验"
    )


class ModelImportResponse(BaseModel):
    """离线模型包导入结果：供设置页展示导入与跳过明细。"""

    imported: list[str] = Field(default_factory=list, description="本次写入（或替换）的模型名")
    skipped: list[str] = Field(default_factory=list, description="包内已就绪、按幂等跳过的模型名")
