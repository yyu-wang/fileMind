"""模型下载规格：仓库 id + 待下载文件 + 落盘目录（下载机制的输入）。

为什么单独成模块：**下载机制**（镜像轮询 / 重试 / 逐字节进度）与**模型来源**是两件
独立的事。两类模型的来源不同，若把它们揉进下载器，每加一类模型都要改动下载主流程：

- Embedding：走 :mod:`app.core.embedding_models` 注册表（``hf_repo`` + ``onnx_file``），
  落盘 ``{models_root}/{model}/{onnx_file}``；
- Rerank：sentence-transformers cross-encoder（无 dim / 表名语义，故不入注册表），
  落盘 ``{models_root}/{model}/``。它是「装到别的机器也能用全部功能」的关键一环：
  全新机器没有 HuggingFace 缓存、hf-mirror 又可能不可达，而
  ``rerank_service.resolve_load_target`` 会优先加载这里的本地副本。

放在 ``app/services`` 而非 ``app/core``：本模块需要 ``embedding_service`` 的路径约定
（``models_root`` / ``model_dir``），而 ``app.core`` 下的模块按约定只依赖 stdlib。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.core.embedding_models import get_model_info
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
    """解析模型的下载规格（Rerank 专用条目优先，其余按 Embedding 注册表）。

    Rerank 的两种写法都接受（本地目录名 ``bge-reranker-v2-m3`` 与仓库 id
    ``BAAI/bge-reranker-v2-m3``），避免各调用方各自记一个名字。

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


def spec_ready(spec: DownloadSpec) -> bool:
    """规格声明的文件是否齐备（存在且非空）——空文件视为「半截下载」。"""
    return all(
        (spec.root / name).is_file() and (spec.root / name).stat().st_size > 0
        for name in spec.files
    )
