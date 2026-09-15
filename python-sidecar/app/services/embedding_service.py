"""T5.1 — 本地 Embedding 生成（Sidecar 进程内 ONNX int8，不依赖 Ollama）。

范围：``embed_text`` / ``embed_texts`` 用 onnxruntime + tokenizers 在本进程内为
文件/查询文本生成向量，是 Epic 5 检索链路（索引构建、增量索引、RAG 查询向量化）
的底层能力，供 ``/index/build`` 与 ``/chat/stream`` 调用。

为什么下沉到进程内：Embedding 没有云端替代方案——安全红线要求原文不出本地，
云端 Provider 的 ``embed`` 一律抛错、Rust 云代理也不暴露 embeddings 路由。
若继续依赖 Ollama，未安装 Ollama 的机器上知识问答整条链路不可用。因此模型
加载与推理改为本进程内完成，Ollama 仅保留「本地生成模型」职责。

为什么用 ONNX 而非 sentence-transformers：ST 会拖入 torch（常驻数百 MB），
而本模块只需一次 BERT 前向 + CLS 池化。实测 int8 权重 311MB、单查询 15ms、
会话加载 328ms（``benchmarks/onnx_embedding_mem_probe.py``）。

模型来源：注册表的 ``hf_repo`` + ``onnx_file``，本地目录布局
``{models_root}/{model}/{onnx_file}``；``models_root`` 默认
``$FILEMIND_DATA_HOME/models``（可用 ``FILEMIND_MODEL_DIR`` 覆盖）。
模型文件由设置页触发下载（``app.services.model_download_service``，T2）；
未下载时本模块抛 :class:`EmbeddingUnavailableError`，由调用方引导用户下载。

错误策略（对齐错误码 EMBEDDING_UNAVAILABLE）：
- 模型未下载 / 加载失败 → 抛 :class:`EmbeddingUnavailableError`（索引不可降级，
  必须中断，由调用方引导用户处理）
- 推理异常 / 超时 / 返回数量异常 → 同样抛
"""

from __future__ import annotations

import asyncio
import os
from pathlib import Path
from typing import TYPE_CHECKING, cast

from app.core.embedding_models import get_model_info

if TYPE_CHECKING:
    import onnxruntime as ort
    from tokenizers import Tokenizer

#: 默认 Embedding 模型（注册表标识）
EMBEDDING_MODEL = os.environ.get("FILEMIND_EMBEDDING_MODEL", "bge-large-zh-v1.5")
#: bge 检索短查询指令前缀（BAAI 模型卡：检索查询需加该指令，文档不加）
QUERY_INSTRUCTION = "为这个句子生成表示以用于检索相关文章："
#: 单次向量化超时（秒），批量输入时给足预算
EMBED_TIMEOUT = float(os.environ.get("FILEMIND_EMBED_TIMEOUT", "60"))
#: encode 批大小（批内按最长序列 padding，过大反而浪费算力与内存）
ENCODE_BATCH_SIZE = int(os.environ.get("FILEMIND_EMBED_BATCH_SIZE", "32"))
#: 单条文本最大 token 数（bge 位置编码上限）
MAX_TOKENS = 512

#: 模型根目录覆盖（指定后直接作为根，不再拼 DATA_HOME）
_ENV_MODEL_DIR = "FILEMIND_MODEL_DIR"
#: 数据目录（与 ``app.main.DATA_HOME`` 同一约定：默认 ``~/.filemind``）
_ENV_DATA_HOME = "FILEMIND_DATA_HOME"
#: ONNX 执行提供者（CPU 单后端：本项目不发 GPU 依赖）
_PROVIDERS = ("CPUExecutionProvider",)


class EmbeddingUnavailableError(Exception):
    """本地 Embedding 不可用（模型未下载 / 加载失败 / 推理异常 / 超时）。"""


#: 模块级惰性单例（会话与 tokenizer 常驻，避免每次向量化重复加载 300MB 权重）
_session: ort.InferenceSession | None = None
_tokenizer: Tokenizer | None = None
_load_lock = asyncio.Lock()


def models_root() -> Path:
    """模型根目录：``FILEMIND_MODEL_DIR`` 优先，否则 ``$FILEMIND_DATA_HOME/models``。

    默认数据目录与 :data:`app.main.DATA_HOME` 保持同一约定（``~/.filemind``），
    保证 Rust 侧注入的环境变量能同时决定数据库与模型的落盘位置。
    """
    override = os.environ.get(_ENV_MODEL_DIR, "").strip()
    if override:
        return Path(override)
    data_home = os.environ.get(_ENV_DATA_HOME, "").strip() or str(Path.home() / ".filemind")
    return Path(data_home) / "models"


def model_dir(model: str = EMBEDDING_MODEL) -> Path:
    """单个模型的本地目录（下载目标 + 加载来源）。

    Raises:
        EmbeddingUnavailableError: 模型未在注册表中（含 env 指向未登记模型的场景）。
    """
    try:
        get_model_info(model)
    except ValueError as exc:
        raise EmbeddingUnavailableError(str(exc)) from exc
    return models_root() / model


def onnx_path(model: str = EMBEDDING_MODEL) -> Path:
    """ONNX 权重的本地绝对路径（``{model_dir}/{onnx_file}``）。"""
    return model_dir(model) / get_model_info(model).onnx_file


def reset_model() -> None:
    """重置会话与 tokenizer 单例（仅测试用）。"""
    global _session, _tokenizer
    _session = None
    _tokenizer = None


def _load(model: str) -> tuple[ort.InferenceSession, Tokenizer]:
    """加载 ONNX 会话与 tokenizer（同步，由调用方下沉线程池）。

    先做文件存在性检查再导入 onnxruntime：模型缺失是本模块最常见的失败，
    提前抛错可给出「未下载」的明确指引，也避免为一次失败导入付出解析开销。

    Raises:
        EmbeddingUnavailableError: 模型文件缺失、onnxruntime/tokenizers 缺失或加载失败。
    """
    weights = onnx_path(model)
    tokenizer_file = model_dir(model) / "tokenizer.json"
    missing = [p for p in (weights, tokenizer_file) if not p.is_file()]
    if missing:
        raise EmbeddingUnavailableError(
            f"Embedding 模型未下载: {model}（缺少 {missing[0]}）；"
            "请在「设置 → Embedding 模型」下载后重试"
        )

    import onnxruntime as ort
    from tokenizers import Tokenizer

    try:
        options = ort.SessionOptions()
        # 关闭内存池与内存复用模式：实测速度不变（159.4 vs 159.5 ms/chunk）
        # 而稳态 RSS 少约 300MB（1197 → 875MB，见 benchmarks/onnx_embedding_mem_probe.py）
        options.enable_cpu_mem_arena = False
        options.enable_mem_pattern = False
        session = ort.InferenceSession(
            str(weights), sess_options=options, providers=list(_PROVIDERS)
        )
        tokenizer = Tokenizer.from_file(str(tokenizer_file))
        tokenizer.enable_truncation(max_length=MAX_TOKENS)
        tokenizer.enable_padding(pad_id=0, pad_token="[PAD]")
    except Exception as exc:  # noqa: BLE001
        raise EmbeddingUnavailableError(
            f"Embedding 模型加载失败: {model}（{exc}）；请在设置页重新下载后重试"
        ) from exc
    return session, tokenizer


async def _get_model(model: str) -> tuple[ort.InferenceSession, Tokenizer]:
    """返回模块级单例（并发安全，双重检查 + asyncio.Lock）。

    Raises:
        EmbeddingUnavailableError: 见 :func:`_load`。
    """
    global _session, _tokenizer
    if _session is not None and _tokenizer is not None:
        return _session, _tokenizer
    async with _load_lock:
        if _session is None or _tokenizer is None:
            _session, _tokenizer = await asyncio.to_thread(_load, model)
    return _session, _tokenizer


def _encode_batch(
    session: ort.InferenceSession,
    tokenizer: Tokenizer,
    texts: list[str],
) -> list[list[float]]:
    """单批 tokenize → ONNX 前向 → CLS 池化 → L2 归一化（同步，CPU 密集）。"""
    import numpy as np

    encodings = tokenizer.encode_batch(texts)
    input_ids = np.array([e.ids for e in encodings], dtype=np.int64)
    attention = np.array([e.attention_mask for e in encodings], dtype=np.int64)
    feed = {
        "input_ids": input_ids,
        "attention_mask": attention,
        # bge 的 token_type 恒为 0（单句输入），显式给出以适配要求该输入的导出图
        "token_type_ids": np.zeros_like(input_ids),
    }
    last_hidden = session.run(None, feed)[0]
    # BGE 用 CLS 池化（首 token），随后归一化：LanceDB 默认 L2 距离而 bge 官方
    # 推荐 cosine，归一化后 L2 排序等价于 cosine——与 search_vectors 的查询侧
    # 归一化（SC-m14）配对，保证索引侧与查询侧同一尺度。
    cls = np.asarray(last_hidden, dtype=np.float32)[:, 0, :]
    norms = np.linalg.norm(cls, axis=1, keepdims=True)
    np.maximum(norms, 1e-12, out=norms)
    # ndarray.tolist() 在 mypy 下解析为 Any，显式断言收敛为声明的返回类型
    return cast("list[list[float]]", (cls / norms).tolist())


def _encode(
    session: ort.InferenceSession,
    tokenizer: Tokenizer,
    texts: list[str],
) -> list[list[float]]:
    """分批发向量化（批内 padding 到最长序列，分批避免长文本拖累整批）。"""
    vectors: list[list[float]] = []
    for start in range(0, len(texts), ENCODE_BATCH_SIZE):
        vectors.extend(_encode_batch(session, tokenizer, texts[start : start + ENCODE_BATCH_SIZE]))
    return vectors


async def embed_texts(texts: list[str], model: str = EMBEDDING_MODEL) -> list[list[float]]:
    """批量生成向量（进程内 ONNX 推理），顺序与输入一致。

    Args:
        texts: 待向量化的文本列表（可为空列表，直接返回空列表）。
        model: Embedding 模型标识（默认取 ``EMBEDDING_MODEL``，须在注册表中）。

    Returns:
        与 ``texts`` 等长的向量列表（每个向量为 ``list[float]``，已 L2 归一化）。

    Raises:
        EmbeddingUnavailableError: 模型未下载/加载失败，或推理异常、超时、
            返回数量与输入不一致。
    """
    if not texts:
        return []
    session, tokenizer = await _get_model(model)
    try:
        vectors = await asyncio.wait_for(
            asyncio.to_thread(_encode, session, tokenizer, texts), timeout=EMBED_TIMEOUT
        )
    except TimeoutError as exc:
        raise EmbeddingUnavailableError(f"Embedding 超时（>{EMBED_TIMEOUT}s）") from exc
    except EmbeddingUnavailableError:
        raise
    except Exception as exc:  # noqa: BLE001
        raise EmbeddingUnavailableError(f"Embedding 调用失败: {exc}") from exc
    if len(vectors) != len(texts):
        raise EmbeddingUnavailableError(
            f"Embedding 返回数量异常: 期望 {len(texts)} 个，实际 {len(vectors)} 个"
        )
    return vectors


async def embed_text(text: str, model: str = EMBEDDING_MODEL) -> list[float]:
    """单条文本生成向量。

    Args:
        text: 待向量化的文本。
        model: Embedding 模型标识（默认取 ``EMBEDDING_MODEL``）。

    Returns:
        该文本的向量（``list[float]``，已 L2 归一化）。

    Raises:
        EmbeddingUnavailableError: 见 :func:`embed_texts`。
    """
    return (await embed_texts([text], model=model))[0]
