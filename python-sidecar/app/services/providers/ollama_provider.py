"""T8.2 — OllamaProvider：本地 Ollama 推理的 LLMProvider 实现。

来源：08_Prompt工程设计 §6（Provider 抽象层）。
把既有 Ollama 调用参数（``think=False``、``format=json``、``temperature``、
``num_predict``）封装进 :class:`~app.services.cloud_provider.LLMProvider`
接口，供 T8.5 服务迁移使用。

复用 ``ollama`` SDK（``AsyncClient``）而非重写 HTTP/SSE 层：与既有调用点
``generation_service.stream_generate`` / ``llm_classify.call_ollama_json``
完全同源，行为一致，避免重复造轮子（规则 20）。

**职责边界**：向量化不在本 Provider 内实现——Embedding 已改为 Sidecar 进程内
ONNX 推理（:mod:`app.services.embedding_service`），不再经 Ollama，故
:meth:`OllamaProvider.embed` 与云端 Provider 一样显式不支持（见其 docstring）。
"""

from __future__ import annotations

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
from app.services.embedding_service import EmbeddingUnavailableError

if TYPE_CHECKING:
    from collections.abc import AsyncIterator


class OllamaProvider(LLMProvider):
    """本地 Ollama 推理 Provider。

    通过 ``ollama.AsyncClient`` 访问 ``OLLAMA_HOST``：
    - :meth:`generate` / :meth:`generate_stream`：``chat``（非流式 / 流式）
    生成均关闭 Qwen3 思维链（``think=False``），对齐既有调用点。
    """

    def __init__(
        self,
        host: str = OLLAMA_HOST,
        model: str = LLM_MODEL,
    ) -> None:
        """初始化。

        Args:
            host: Ollama 服务地址（默认取 ``FILEMIND_OLLAMA_URL``）。
            model: 生成模型名（默认取 ``FILEMIND_LLM_MODEL``）。
        """
        self._client = AsyncClient(host=host)
        self._model = model

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
        except (httpx.HTTPError, ConnectionError, ResponseError) as exc:
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
        except (httpx.HTTPError, ConnectionError, ResponseError) as exc:
            raise LLMUnavailableError(f"Ollama 流式生成失败: {exc}") from exc

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """显式不支持：向量化已统一为 Sidecar 进程内 ONNX 推理。

        Ollama 不再承担 Embedding 职责（未安装 Ollama 的机器也要能做知识问答），
        且同一份向量不能混用不同后端的输出，故这里与云端 Provider 一样直接抛错，
        避免调用方误以为还存在「Ollama 向量化」路径。

        Args:
            texts: 待向量化文本列表（忽略）。
            **kwargs: 忽略。

        Raises:
            EmbeddingUnavailableError: 恒定抛出，指引改用 ``embedding_service``。
        """
        raise EmbeddingUnavailableError(
            "OllamaProvider 不再提供向量化：请在 app.services.embedding_service "
            "中调用 embed_texts/embed_text（进程内 ONNX 推理）"
        )
