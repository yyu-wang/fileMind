"""T8.2 — OllamaProvider：本地 Ollama 推理的 LLMProvider 实现。

来源：08_Prompt工程设计 §6（Provider 抽象层）。
把既有 Ollama 调用参数（``think=False``、``format=json``、``temperature``、
``num_predict``）封装进 :class:`~app.services.cloud_provider.LLMProvider`
接口，供 T8.5 服务迁移使用。

复用 ``ollama`` SDK（``AsyncClient``）而非重写 HTTP/SSE 层：与既有调用点
``generation_service.stream_generate`` / ``llm_classify.call_ollama_json`` /
``embedding_service.embed_texts`` 完全同源，行为一致，避免重复造轮子（规则 20）。
"""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

import httpx
from ollama import AsyncClient, Message, Options, ResponseError

from app.rules.llm_classify import (
    LLM_MODEL,
    OLLAMA_HOST,
    OLLAMA_KEEP_ALIVE,
    OLLAMA_NUM_CTX,
    LLMUnavailableError,
)
from app.services.cloud_provider import LLMProvider
from app.services.embedding_service import (
    EMBED_TIMEOUT,
    EMBEDDING_MODEL,
    EmbeddingUnavailableError,
    _ollama_model_name,
)

if TYPE_CHECKING:
    from collections.abc import AsyncIterator


class OllamaProvider(LLMProvider):
    """本地 Ollama 推理 Provider。

    通过 ``ollama.AsyncClient`` 访问 ``OLLAMA_HOST``：
    - :meth:`generate` / :meth:`generate_stream`：``chat``（非流式 / 流式）
    - :meth:`embed`：``embed``（批量向量化）
    生成均关闭 Qwen3 思维链（``think=False``），对齐既有调用点。
    """

    def __init__(
        self,
        host: str = OLLAMA_HOST,
        model: str = LLM_MODEL,
        embed_model: str = EMBEDDING_MODEL,
    ) -> None:
        """初始化。

        Args:
            host: Ollama 服务地址（默认取 ``FILEMIND_OLLAMA_URL``）。
            model: 生成模型名（默认取 ``FILEMIND_LLM_MODEL``）。
            embed_model: 向量化模型注册表标识（默认取 ``FILEMIND_EMBEDDING_MODEL``）。
        """
        self._client = AsyncClient(host=host)
        self._model = model
        self._embed_model = embed_model

    async def generate(
        self,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        json_mode: bool = False,
        **kwargs: object,
    ) -> str:
        """非流式补全（对齐 ``call_ollama_json`` 的调用参数）。

        Args:
            system: system 提示词。
            user: user 提示词。
            temperature: 采样温度。
            max_tokens: 最大输出 token 数（映射 ``num_predict``；``None`` 用后端默认）。
            json_mode: 是否约束 JSON 输出（映射 ``format="json"``）。
            **kwargs: 透传（当前忽略，保留接口扩展位）。

        Returns:
            完整补全文本（不含思考链 token）。

        Raises:
            LLMUnavailableError: Ollama 连接 / HTTP 失败。
        """
        try:
            resp = await self._client.chat(
                model=self._model,
                messages=[
                    Message(role="system", content=system),
                    Message(role="user", content=user),
                ],
                format="json" if json_mode else None,
                think=False,
                # keep_alive 是 chat 顶层参数（模型常驻），num_ctx 在 Options 里
                keep_alive=OLLAMA_KEEP_ALIVE,
                options=Options(
                    temperature=temperature,
                    num_predict=max_tokens,
                    num_ctx=OLLAMA_NUM_CTX,
                ),
            )
        except (httpx.HTTPError, ResponseError) as exc:
            raise LLMUnavailableError(f"Ollama 调用失败: {exc}") from exc
        message = resp.message
        if message is None:
            return ""
        content = message.content
        return content if isinstance(content, str) else ""

    async def generate_stream(
        self,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        **kwargs: object,
    ) -> AsyncIterator[str]:
        """流式补全，逐 delta yield（对齐 ``generation_service.stream_generate``）。

        参数与 :meth:`generate` 一致。

        Yields:
            非空内容 delta（Ollama 逐 token 推送）。

        Raises:
            LLMUnavailableError: 首个 delta 前 Ollama 连接 / HTTP 失败。
        """
        try:
            # ollama SDK 在 stream=True 时重载为 AsyncIterator[ChatResponse]，
            # 无需显式 cast（mypy 可推断）。
            stream = await self._client.chat(
                model=self._model,
                messages=[
                    Message(role="system", content=system),
                    Message(role="user", content=user),
                ],
                stream=True,
                think=False,
                keep_alive=OLLAMA_KEEP_ALIVE,
                options=Options(
                    temperature=temperature,
                    num_predict=max_tokens,
                    num_ctx=OLLAMA_NUM_CTX,
                ),
            )
            async for chunk in stream:
                message = chunk.message
                if message is None:
                    continue
                content = message.content
                if content:
                    yield content
        except (httpx.HTTPError, ResponseError) as exc:
            raise LLMUnavailableError(f"Ollama 流式生成失败: {exc}") from exc

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """批量向量化，顺序与输入一致（对齐 ``embedding_service.embed_texts``）。

        Args:
            texts: 待向量化文本列表（可为空列表，直接返回空列表）。
            **kwargs: 透传（当前忽略，模型名固定走 ``embed_model``）。

        Returns:
            与 ``texts`` 等长的向量列表。

        Raises:
            EmbeddingUnavailableError: Ollama 不可用 / 模型未拉取 / 超时。
        """
        if not texts:
            return []
        model = self._embed_model
        try:
            resp = await asyncio.wait_for(
                # keep_alive 是 embed 顶层参数（模型常驻避免重复冷加载）；
                # embedding 模型无上下文窗口概念，不传 num_ctx。
                self._client.embed(
                    model=_ollama_model_name(model),
                    input=texts,
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
        except httpx.HTTPError as exc:
            raise EmbeddingUnavailableError(f"Embedding 调用失败: {exc}") from exc
        except TimeoutError as exc:
            raise EmbeddingUnavailableError(f"Embedding 超时（>{EMBED_TIMEOUT}s）") from exc

        embeddings = resp.embeddings
        if not isinstance(embeddings, list) or len(embeddings) != len(texts):
            raise EmbeddingUnavailableError(
                f"Embedding 返回数量异常: 期望 {len(texts)} 个，实际 {len(embeddings)} 个"
            )
        return [list(v) for v in embeddings]
