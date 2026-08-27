"""T5.1 — Ollama Embedding 生成封装。

范围：``embed_text`` / ``embed_texts`` 通过 Ollama Embedding API 为文件/查询
文本生成向量。是 Epic 5 检索链路（索引构建、增量索引、RAG 查询向量化）的底层
能力，供后续长任务与 /chat/stream 调用。

模型名说明：``bge-large-zh-v1.5`` 不在 Ollama 官方库，由社区命名空间托底
（``qllama/bge-large-zh-v1.5``，与官方模型同构、实测 1024 维）。业务侧仍用
注册表标识（``app/core/embedding_models.py`` 的 key，用于 API 参数 / LanceDB
表名），本模块负责解析为 Ollama 实际模型名。

错误策略（对齐 API 规格书错误码 OLLAMA_UNAVAILABLE）：
- 连接 / HTTP / 超时失败 → 抛 :class:`EmbeddingUnavailableError`（索引不可
  降级，必须中断，由调用方引导用户处理）
- 模型未拉取（Ollama 对未知模型返回 404）→ 同样抛，但消息提示先 pull
"""

from __future__ import annotations

import asyncio
import os

import httpx
from ollama import AsyncClient, ResponseError

from app.rules.llm_classify import OLLAMA_KEEP_ALIVE

#: Ollama 服务地址（env 可覆盖，与 llm_classify.py 共用同一环境变量）
OLLAMA_HOST = os.environ.get("FILEMIND_OLLAMA_URL", "http://127.0.0.1:11434")
#: 默认 Embedding 模型（注册表标识；env 可覆盖为任意 Ollama 模型名）
EMBEDDING_MODEL = os.environ.get("FILEMIND_EMBEDDING_MODEL", "bge-large-zh-v1.5")
#: bge 检索短查询指令前缀（BAAI bge-large-zh-v1.5 模型卡：检索查询需加该指令，文档不加）
QUERY_INSTRUCTION = "为这个句子生成表示以用于检索相关文章："
#: 单次 Embedding 调用超时（秒），批量输入时给足预算
EMBED_TIMEOUT = float(os.environ.get("FILEMIND_EMBED_TIMEOUT", "60"))

#: 注册表模型标识 → Ollama 实际模型名（bge 系列非官方库，社区命名空间托底）
_OLLAMA_MODEL_ALIASES: dict[str, str] = {
    "bge-large-zh-v1.5": "qllama/bge-large-zh-v1.5",
}


class EmbeddingUnavailableError(Exception):
    """Ollama Embedding 服务不可用（连接失败 / HTTP 错误 / 超时 / 模型缺失）。"""


#: 模块级惰性单例 AsyncClient（httpx 连接池复用，避免每次 embedding 重建连接）
_client: AsyncClient | None = None
_client_lock = asyncio.Lock()


async def _get_client() -> AsyncClient:
    """返回模块级 AsyncClient 单例（并发安全，双重检查 + asyncio.Lock）。"""
    global _client
    if _client is None:
        async with _client_lock:
            if _client is None:
                _client = AsyncClient(host=OLLAMA_HOST)
    return _client


def reset_clients() -> None:
    """重置客户端单例（仅测试用）。"""
    global _client
    _client = None


def _ollama_model_name(model: str) -> str:
    """把注册表标识解析为 Ollama 实际模型名；无别名时原样返回（支持 env 直接覆盖）。"""
    return _OLLAMA_MODEL_ALIASES.get(model, model)


async def embed_texts(texts: list[str], model: str = EMBEDDING_MODEL) -> list[list[float]]:
    """批量生成向量（一次 Ollama 请求），顺序与输入一致。

    Args:
        texts: 待向量化的文本列表（可为空列表，直接返回空列表）。
        model: Embedding 模型名（默认取 ``EMBEDDING_MODEL``）。

    Returns:
        与 ``texts`` 等长的向量列表（每个向量为 ``list[float]``）。

    Raises:
        EmbeddingUnavailableError: Ollama 不可用、模型未拉取或调用超时。
    """
    if not texts:
        return []
    client = await _get_client()
    try:
        resp = await asyncio.wait_for(
            client.embed(
                model=_ollama_model_name(model),
                input=texts,
                # keep_alive 是 embed 顶层参数（模型常驻避免重复冷加载）；
                # embedding 模型无上下文窗口概念，不传 num_ctx。
                keep_alive=OLLAMA_KEEP_ALIVE,
            ),
            timeout=EMBED_TIMEOUT,
        )
    except ResponseError as exc:
        if exc.status_code == 404:
            raise EmbeddingUnavailableError(
                f"Embedding 模型未拉取: {model!r}（请先运行 ollama pull "
                f"{_ollama_model_name(model)}）"
            ) from exc
        raise EmbeddingUnavailableError(f"Embedding 调用失败: {exc}") from exc
    except (httpx.HTTPError, ConnectionError) as exc:
        # ollama SDK 在连接失败时抛出内置 ConnectionError（非 httpx.HTTPError），
        # 需显式捕获，否则异常逃逸到 ASGI 层导致 SSE 流断裂、前端看门狗超时。
        raise EmbeddingUnavailableError(f"Embedding 调用失败: {exc}") from exc
    except TimeoutError as exc:
        raise EmbeddingUnavailableError(f"Embedding 超时（>{EMBED_TIMEOUT}s）") from exc

    embeddings = resp.embeddings
    if not isinstance(embeddings, list) or len(embeddings) != len(texts):
        raise EmbeddingUnavailableError(
            f"Embedding 返回数量异常: 期望 {len(texts)} 个，实际 {len(embeddings)} 个"
        )
    return [list(v) for v in embeddings]


async def embed_text(text: str, model: str = EMBEDDING_MODEL) -> list[float]:
    """单条文本生成向量。

    Args:
        text: 待向量化的文本。
        model: Embedding 模型名（默认取 ``EMBEDDING_MODEL``）。

    Returns:
        该文本的向量（``list[float]``）。

    Raises:
        EmbeddingUnavailableError: 见 :func:`embed_texts`。
    """
    return (await embed_texts([text], model=model))[0]
