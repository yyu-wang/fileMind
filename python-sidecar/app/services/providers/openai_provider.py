"""T8.3 — OpenAIProvider：经本地 cloud_proxy 访问 OpenAI 云端推理。

来源：08_Prompt工程设计 §6（Provider 抽象层）/ T8.3 任务标题。

密钥安全架构（T7.3/T7.4）：云端 API Key 存 Rust Keychain，Sidecar 把 OpenAI
Chat Completions 请求体 POST 到本地代理 ``127.0.0.1:8766/cloud-proxy/openai``，
代理从 Keychain 注入 ``Authorization: Bearer`` 后转发，全程不经过 Python。
因此本 Provider 用 ``api_key="filemind-proxy"`` 占位（仅满足 SDK 非空约束），
真实鉴权走 ``x-filemind-token`` 共享 token（env ``FILEMIND_CLOUD_PROXY_TOKEN``）。

与 OllamaProvider 的差异：
- 错误映射到 :class:`CloudUnavailableError`（云端域错误，T8.4 共用）
- ``embed`` 明确报错：代理仅暴露 ``/chat/completions``，无 embeddings 路由，
  检索 Embedding 固定本地 bge（E5 门控已验证 98% 召回）
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

from openai import AsyncOpenAI, Omit, OpenAIError
from openai.types.chat import ChatCompletionSystemMessageParam, ChatCompletionUserMessageParam
from openai.types.shared_params import ResponseFormatJSONObject

from app.services.cloud_provider import (
    CloudUnavailableError,
    LLMProvider,
    PromptVersion,
)

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

#: 本地 cloud_proxy 监听地址（cloud_proxy.rs::CLOUD_PROXY_HOST/PORT）
CLOUD_PROXY_BASE_URL = "http://127.0.0.1:8766"
#: 生成模型名（env 可覆盖；与 09 测试文档「云端模式 OpenAI gpt-4o」一致）
OPENAI_MODEL = os.environ.get("FILEMIND_OPENAI_MODEL", "gpt-4o")
#: 云端单次调用超时（秒）：生成可能较慢，网络 + 模型推理留足余量
CLOUD_TIMEOUT_S = 120.0
#: 代理共享 token 的 env 键（cloud_proxy.rs 鉴权头同名）
_CLOUD_TOKEN_ENV = "FILEMIND_CLOUD_PROXY_TOKEN"


class OpenAIProvider(LLMProvider):
    """OpenAI 云端推理 Provider（经本地 cloud_proxy）。

    - :meth:`generate` / :meth:`generate_stream`：``chat.completions``（非流式 / 流式）
    - :meth:`embed`：抛 :class:`CloudUnavailableError`（云端 Embedding 未启用）

    ``version = "cloud"``：云端模型利用 ``response_format`` 强约束，prompt 可精简
    （T8.5 调用点据此选择云端变体；DeepSeekProvider 继承本属性）。
    """

    version: PromptVersion = "cloud"

    def __init__(
        self,
        base_url: str = f"{CLOUD_PROXY_BASE_URL}/cloud-proxy/openai",
        model: str = OPENAI_MODEL,
        token: str | None = None,
        timeout: float = CLOUD_TIMEOUT_S,
        max_retries: int = 1,
        default_max_tokens: int | None = None,
    ) -> None:
        """初始化。

        Args:
            base_url: 本地代理的 OpenAI 兼容端点（SDK 自动拼 ``/chat/completions``）。
            model: 云端模型名。
            token: 代理共享 token；``None`` 时取 ``FILEMIND_CLOUD_PROXY_TOKEN``。
            timeout: 单次调用超时（秒）。
            max_retries: SDK 重试次数（云端偶发抖动，保守设 1）。
            default_max_tokens: provider 级默认输出上限；调用方未显式指定
                ``max_tokens``（``None``）时使用。``None`` 表示沿用后端默认。
                子类（如 DeepSeek 中文长回答防截断）可设非 ``None``。
        """
        self._model = model
        self._default_max_tokens = default_max_tokens
        self._client = AsyncOpenAI(
            base_url=base_url,
            api_key="filemind-proxy",  # 占位：真实 Key 由代理从 Keychain 注入
            default_headers={"x-filemind-token": token or os.environ.get(_CLOUD_TOKEN_ENV, "")},
            timeout=timeout,
            max_retries=max_retries,
        )

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
        """非流式补全（Chat Completions，经本地代理）。

        参数与 :meth:`~app.services.cloud_provider.LLMProvider.generate` 一致；
        ``json_mode`` 映射 ``response_format={"type": "json_object"}``。

        Raises:
            CloudUnavailableError: 代理不可达 / HTTP 错误 / 鉴权失败 / 超时。
        """
        messages = self._build_messages(system, user)
        response_format = ResponseFormatJSONObject(type="json_object") if json_mode else Omit()
        # 调用方未显式指定时回落 provider 级默认（DeepSeek 中文长回答防截断）
        effective_max_tokens = max_tokens if max_tokens is not None else self._default_max_tokens
        try:
            resp = await self._client.chat.completions.create(
                model=self._model,
                messages=messages,
                temperature=temperature,
                max_tokens=effective_max_tokens,
                response_format=response_format,
            )
        except OpenAIError as exc:
            raise CloudUnavailableError(f"OpenAI 调用失败: {exc}") from exc
        choices = resp.choices
        if not choices:
            return ""
        content = choices[0].message.content
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
        """流式补全，逐 delta yield（SDK AsyncStream，代理 SSE 透传）。

        参数与 :meth:`generate` 一致（流式场景不使用 ``json_mode``）。

        Yields:
            非空内容 delta。

        Raises:
            CloudUnavailableError: 首个 delta 前代理不可达 / HTTP 错误 / 超时。
        """
        messages = self._build_messages(system, user)
        effective_max_tokens = max_tokens if max_tokens is not None else self._default_max_tokens
        try:
            stream = await self._client.chat.completions.create(
                model=self._model,
                messages=messages,
                temperature=temperature,
                max_tokens=effective_max_tokens,
                stream=True,
            )
        except OpenAIError as exc:
            raise CloudUnavailableError(f"OpenAI 流式调用失败: {exc}") from exc
        try:
            async for chunk in stream:
                if not chunk.choices:
                    continue
                delta = chunk.choices[0].delta
                if delta is None:
                    continue
                content = delta.content
                if content:
                    yield content
        except OpenAIError as exc:
            raise CloudUnavailableError(f"OpenAI 流式生成失败: {exc}") from exc

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """云端 Embedding 未启用。

        cloud_proxy 仅暴露 ``/chat/completions``，无 embeddings 路由；检索
        Embedding 固定本地 bge（E5 门控已验证）。抛 :class:`CloudUnavailableError`
        保持接口完整、语义诚实。

        Raises:
            CloudUnavailableError: 始终（云端 Embedding 未启用）。
        """
        raise CloudUnavailableError(
            "云端 Embedding 未启用：本地代理不暴露 embeddings 路由，检索固定使用本地 bge"
        )

    @staticmethod
    def _build_messages(
        system: str, user: str
    ) -> list[ChatCompletionSystemMessageParam | ChatCompletionUserMessageParam]:
        """构造 OpenAI 消息数组（system + user）。"""
        return [
            ChatCompletionSystemMessageParam(role="system", content=system),
            ChatCompletionUserMessageParam(role="user", content=user),
        ]
