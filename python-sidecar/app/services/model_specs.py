"""模型下载规格：仓库 id + 待下载文件 + 落盘目录（下载机制的输入）。

为什么单独成模块：**下载机制**（镜像轮询 / 重试 / 逐字节进度）与**模型来源**是两件
独立的事。三类模型的来源不同，若把它们揉进下载器，每加一类模型都要改动下载主流程：

- Embedding：走 :mod:`app.core.embedding_models` 注册表（``hf_repo`` + ``onnx_file``），
  落盘 ``{models_root}/{model}/{onnx_file}``；
- Rerank：sentence-transformers cross-encoder（无 dim / 表名语义，故不入注册表），
  落盘 ``{models_root}/{model}/``。它是「装到别的机器也能用全部功能」的关键一环：
  全新机器没有 HuggingFace 缓存、hf-mirror 又可能不可达，而
  ``rerank_service.resolve_load_target`` 会优先加载这里的本地副本；
- 本地生成模型（GGUF）：内置 llama.cpp 引擎的权重（T3），单文件 GGUF 落盘
  ``{models_root}/{model}/``。它是「未安装 Ollama 的机器也能知识问答」的最后一块
  拼图，同样复用同一条下载管线（镜像轮询 / 重试 / 进度）。

离线导入（:mod:`app.services.model_import_service`）按 :func:`importable_models`
遍历本模块的三类来源，故新增模型无需改动导入逻辑。

放在 ``app/services`` 而非 ``app/core``：本模块需要 ``embedding_service`` 的路径约定
（``models_root`` / ``model_dir``），而 ``app.core`` 下的模块按约定只依赖 stdlib。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.core.embedding_models import get_model_info, list_model_names
from app.services.embedding_service import model_dir, models_root

if TYPE_CHECKING:
    from pathlib import Path

#: Embedding 权重之外还需的仓库文件（tokenizer 与配置；体积 KB 级）
_EMBEDDING_AUX_FILES: tuple[str, ...] = (
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "vocab.txt",
    "config.json",
)

#: Rerank 模型的本地目录名 / 对外模型名（与 Embedding 模型名同一命名口径）
RERANK_MODEL_NAME = "bge-reranker-v2-m3"
#: Rerank 仓库 id（本地目录的来源标注，也是 sentence-transformers 的加载标识）
RERANK_REPO = "BAAI/bge-reranker-v2-m3"
#: Rerank 权重文件（进度统计基准；safetensors 占体积绝大部分）
RERANK_WEIGHT_FILE = "model.safetensors"
#: Rerank 权重之外的必需文件：config + tokenizer
#: （XLM-R 骨干用 sentencepiece，另带 json 分词器以兼容新版 transformers）
_RERANK_AUX_FILES: tuple[str, ...] = (
    "config.json",
    "sentencepiece.bpe.model",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
)

#: 本地生成模型（GGUF）：内置 llama.cpp 引擎的权重来源（T3）。
#:
#: 与 Rerank 同理——没有 dim / 表名语义，故不进 Embedding 注册表；权重是单文件
#: GGUF，落盘 ``{models_root}/{model}/{model}-{quant}.gguf``。
#:
#: 目录名取**模型名**而非文件名：同一模型的不同量化档（q4_k_m / q5_k_m …）落在
#: 同一目录下共存，切换档位不必重下整份目录结构；文件名字面沿用 HuggingFace
#: 仓库内命名，便于人工核对下载 URL。
LLM_MODEL_NAME = "qwen2.5-3b-instruct"
#: 默认量化档（Q4_K_M：约 2GB，质量与体积的平衡点，CPU 可跑）
LLM_QUANT = "q4_k_m"
#: GGUF 权重文件名（仓库内相对路径，也是进度统计基准）
LLM_WEIGHT_FILE = f"{LLM_MODEL_NAME}-{LLM_QUANT}.gguf"
#: GGUF 仓库 id（Qwen 官方仓库，含 q2_k~q8_0 各量化档）
LLM_REPO = "Qwen/Qwen2.5-3B-Instruct-GGUF"
#: 设置页展示用的可读标签
LLM_MODEL_LABEL = "Qwen2.5-3B-Instruct · Q4_K_M"


@dataclass(frozen=True)
class DownloadSpec:
    """一次下载任务的静态规格：模型从哪来、要哪些文件、落到哪个目录。"""

    repo: str
    """HuggingFace 仓库 id（镜像 URL 与本地目录的来源标注）。"""

    files: tuple[str, ...]
    """仓库内需要下载的相对路径（权重 + tokenizer/配置）。"""

    weight: str
    """权重文件相对路径——进度统计基准（体积占 99.99%，且大小可稳定探测）。"""

    root: Path
    """落盘根目录（文件按 ``root/{相对路径}`` 存放）。"""


def resolve_spec(model: str) -> DownloadSpec:
    """解析模型的下载规格（Rerank / GGUF 专用条目优先，其余按 Embedding 注册表）。

    非 Embedding 的两类都接受两种写法（本地目录名与仓库 id），避免各调用方
    各自记一个名字。

    Raises:
        ValueError: 模型不在任一来源中（调用方转 HTTP 400）。
        EmbeddingUnavailableError: 数据目录不可解析（Embedding 侧）。
    """
    if model in (RERANK_MODEL_NAME, RERANK_REPO):
        return DownloadSpec(
            repo=RERANK_REPO,
            files=(RERANK_WEIGHT_FILE, *_RERANK_AUX_FILES),
            weight=RERANK_WEIGHT_FILE,
            root=models_root() / RERANK_MODEL_NAME,
        )
    if model in (LLM_MODEL_NAME, LLM_REPO):
        return DownloadSpec(
            repo=LLM_REPO,
            # 单文件权重：GGUF 自带词表与超参，没有 tokenizer/config 辅助文件
            files=(LLM_WEIGHT_FILE,),
            weight=LLM_WEIGHT_FILE,
            root=models_root() / LLM_MODEL_NAME,
        )
    info = get_model_info(model)
    return DownloadSpec(
        repo=info.hf_repo,
        files=(info.onnx_file, *_EMBEDDING_AUX_FILES),
        weight=info.onnx_file,
        root=model_dir(model),
    )


def model_dir_for(model: str) -> Path:
    """模型文件的落盘目录（= ``resolve_spec(model).root``）。

    Raises:
        ValueError: 模型不在任一来源中。
    """
    return resolve_spec(model).root


def importable_models() -> list[str]:
    """可离线导入的模型名（Embedding 注册表 + Rerank + GGUF），顺序稳定。

    三类来源各自的清单都只在这里拼装一次：离线导入的「包内允许出现哪些模型目录」
    与「期望模型提示语」共用本函数，避免新增模型时漏改其中一处。
    """
    return [*list_model_names(), RERANK_MODEL_NAME, LLM_MODEL_NAME]


def spec_ready(spec: DownloadSpec) -> bool:
    """规格声明的文件是否齐备（存在且非空）——空文件视为「半截下载」。"""
    return all(
        (spec.root / name).is_file() and (spec.root / name).stat().st_size > 0
        for name in spec.files
    )
