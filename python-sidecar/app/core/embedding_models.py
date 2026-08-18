"""Embedding 模型注册表：名 → 维度/默认版本/描述。

集中维护：避免 routes/services 里散落硬编码 dim。新增模型不需要重跑迁移，
建 LanceDB 新表（``documents_{model}_v{N}``）即可，对齐 ``rules/sql.md``。

注意：本模块只依赖 Python stdlib，不 import 任何 db/service。版本分配逻辑
在 :mod:`app.services.embedding_switch_service` 实现（它可以调用 LanceDB
查表名列表）。
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class EmbeddingModelInfo:
    """单个 Embedding 模型的注册信息（不可变）。"""

    name: str
    """模型唯一标识（用于 API 参数 / 表名拼装）。"""

    dim: int
    """输出向量维度（影响 LanceDB schema 锚点长度）。"""

    default_version: int
    """首次使用时分配的默认版本号（通常 = 1）。"""

    description: str
    """人类可读描述（前端展示用）。"""


# 已上线支持的模型列表。加模型 = 加一条记录。
MODEL_REGISTRY: dict[str, EmbeddingModelInfo] = {
    "bge-large-zh-v1.5": EmbeddingModelInfo(
        name="bge-large-zh-v1.5",
        dim=1024,
        default_version=1,
        description="BAAI 通用中文大模型（1024维，质量优先）",
    ),
    "bge-m3": EmbeddingModelInfo(
        name="bge-m3",
        dim=1024,
        default_version=1,
        description="BAAI 多语言多粒度通用检索（1024维，中英混搜）",
    ),
    "bge-small-zh-v1.5": EmbeddingModelInfo(
        name="bge-small-zh-v1.5",
        dim=512,
        default_version=1,
        description="BAAI 轻量中文检索模型（512维，速度优先）",
    ),
}

# 启动默认模型：等价于 settings 的 fallback，也被 lifespan 写入 state
DEFAULT_MODEL: str = "bge-large-zh-v1.5"

# 速度估算：粗估 500 文件/分钟（CPU + Ollama 在消费级机器上的保守值）
# 用于预检查阶段的 est_minutes 估算；真实运行中会按已耗时间重算
EST_FILES_PER_MINUTE = 500
# 预估耗时上限（999 分钟 = 16.65h 足够大文件库）
EST_MINUTES_CAP = 999


def list_model_names() -> list[str]:
    """返回所有已注册模型名（按 key 字典序，前端展示稳定）。"""
    return sorted(MODEL_REGISTRY.keys())


def get_model_info(model: str) -> EmbeddingModelInfo:
    """根据模型名返回注册信息；不存在抛 :class:`ValueError`。

    捕获方：FastAPI 路由层转 HTTPException(400)。
    """
    info = MODEL_REGISTRY.get(model)
    if info is None:
        raise ValueError(f"未知 Embedding 模型: {model!r}（可用: {list_model_names()}）")
    return info


def get_model_dim(model: str) -> int:
    """``get_model_info(model).dim`` 的快捷入口。"""
    return get_model_info(model).dim
