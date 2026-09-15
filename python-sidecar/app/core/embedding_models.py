"""Embedding 模型注册表：名 → 维度/默认版本/HF 来源/ONNX 权重/描述。

集中维护：避免 routes/services 里散落硬编码 dim 与模型路径。新增模型不需要
重跑迁移，建 LanceDB 新表（``documents_{model}_v{N}``）即可，对齐 ``rules/sql.md``。

注意：本模块只依赖 Python stdlib，不 import 任何 db/service。版本分配逻辑
在 :mod:`app.services.embedding_switch_service` 实现（它可以调用 LanceDB
查表名列表）。

向量化已改为 **Sidecar 进程内 ONNX 推理**（见 :mod:`app.services.embedding_service`）：
不再经 Ollama，因此每个模型必须给出 ``hf_repo`` 与 ``onnx_file``——前者是
模型下载源（``app.services.model_download_service``），后者是仓库内 ONNX
权重的相对路径，两者共同决定本地模型目录布局
（``{models_root}/{model}/{onnx_file}``）。
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class EmbeddingModelInfo:
    """单个 Embedding 模型的注册信息（不可变）。"""

    name: str
    """模型唯一标识（用于 API 参数 / 表名拼装 / 本地目录名）。"""

    dim: int
    """输出向量维度（影响 LanceDB schema 锚点长度）。"""

    default_version: int
    """首次使用时分配的默认版本号（通常 = 1）。"""

    hf_repo: str
    """HuggingFace 仓库 id（模型下载源，也是本地目录的来源标注）。"""

    onnx_file: str
    """仓库内 ONNX 权重相对路径（int8 量化版；本地按同一相对路径落盘）。"""

    description: str
    """人类可读描述（前端展示用）。"""


# 已上线支持的模型列表。加模型 = 加一条记录。
# 只保留 bge-large：embedding 改进程内 ONNX 后，原先作为「备选/轻量」存在的
# bge-m3 与 bge-small-zh-v1.5 不再提供（dim 变更会强制全量重建，收益不足）。
MODEL_REGISTRY: dict[str, EmbeddingModelInfo] = {
    "bge-large-zh-v1.5": EmbeddingModelInfo(
        name="bge-large-zh-v1.5",
        dim=1024,
        # 版本保持 1：向量后端由「Ollama GGUF」换为「进程内 ONNX int8」后，曾评估
        # 升版本（新表 documents_*_v2）以避免两套向量混用；但实测旧库里不存在
        # bge-large 的旧后端向量（历史配置为 bge-small，其表名与归一闪开后不同），
        # 归一闪开本身即触发全量重嵌，升版本只会让所有用户多跑一次全量重建。
        # 若未来确需升版本，注意三处表名构造必须同步（Rust 侧已统一走
        # commands::embedding_table::resolve_vector_table，不再硬编码 v1）。
        default_version=1,
        hf_repo="Xenova/bge-large-zh-v1.5",
        onnx_file="onnx/model_quantized.onnx",
        description="BAAI 通用中文大模型（1024维，质量优先；ONNX int8 进程内推理）",
    ),
}

# 启动默认模型：等价于 settings 的 fallback，也被 lifespan 写入 state
DEFAULT_MODEL: str = "bge-large-zh-v1.5"

# 速度估算：粗估 100 文件/分钟（进程内 ONNX int8 实测标定，用于预检查阶段的
# est_minutes；真实运行中会按已耗时间重算）。
# 依据：M1 Pro + ORT 1.30，500 字分块（生产分块尺寸）+ 批 20 实测 202.6 分块/分钟，
# 按平均 2 分块/文件折算 ≈ 101 文件/分钟（见 benchmarks/onnx_embedding_mem_probe.py）。
# 注意：批量加大（40/80）无收益（201/198 分块/分钟），故沿用 ingest 的 EMBED_BATCH_SIZE=20。
# 旧值 500 是 Ollama 时代的粗估，对 CPU int8 ONNX 明显偏乐观（约 5 倍）。
EST_FILES_PER_MINUTE = 100
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
