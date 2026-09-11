"""P-07 — GenericCloudProvider：任意 OpenAI 兼容云端提供商通用实现。

来源：P-07 自定义提供商改造。用户在前端设置页以表单写入
``cloud_providers.provider_key``（slug，如 ``qwen``、``moonshot``、``glm``），
Rust 云端代理据此 slug 查 DB 取真实 ``base_url`` 并拼 ``/chat/completions``
后转发。Python 端完全不持有具体提供商 URL，仅把 slug 放在代理路由尾段
``/cloud-proxy/{provider_slug}``，满足 07-§4「URL 真源在 Rust」的安全约束。

行为与 :class:`~app.services.providers.openai_provider.OpenAIProvider` 等价：
- 请求格式、JSON mode、SSE 流式、错误映射全部继承
- ``version = "cloud"``（用户定义的 provider 都属于云端，使用云端 prompt 变体）
- ``embed`` 默认与 OpenAI 相同：未启用（Embedding 固定本地 bge）

参数来源：
- ``provider_slug`` 通过构造参数传入；缺省读取 env ``FILEMIND_ACTIVE_CLOUD_PROVIDER``
  （Rust 启动 Sidecar 时按 ``app_config.active_cloud_provider`` 注入）
- 默认模型名：env ``FILEMIND_CLOUD_MODEL``（用户 UI 表单中填入模型），缺省回落
  ``gpt-4o-mini``（与现有云端回落值一致，仅占位，真实模型由用户显式指定）
"""

from __future__ import annotations

import os

from app.services.cloud_provider import CloudUnavailableError, PromptVersion
from app.services.providers.openai_provider import (
    CLOUD_PROXY_BASE_URL,
    CLOUD_TIMEOUT_S,
    OpenAIProvider,
)

#: 默认云模型名（仅 GenericCloudProvider 使用；调用方显式传 model 时覆盖）
_DEFAULT_CLOUD_MODEL = os.environ.get("FILEMIND_CLOUD_MODEL", "gpt-4o-mini")
#: Rust 启动 Sidecar 时注入的激活提供商 slug（对应 app_config.active_cloud_provider）
_ACTIVE_PROVIDER_ENV = "FILEMIND_ACTIVE_CLOUD_PROVIDER"


def _default_proxy_endpoint(provider_slug: str) -> str:
    """构造本地代理端点：``/cloud-proxy/{slug}``。

    ``OpenAIProvider`` 会在其后由 SDK 自动拼 ``/chat/completions``，正好对齐
    Rust 云端代理的 ``POST /cloud-proxy/:provider`` 路由。

    Args:
        provider_slug: 提供商 slug（与 DB ``cloud_providers.provider_key`` 一致）。

    Returns:
        传给 SDK 的 base_url。
    """
    cleaned = provider_slug.strip("/")
    return f"{CLOUD_PROXY_BASE_URL}/cloud-proxy/{cleaned}"


class GenericCloudProvider(OpenAIProvider):
    """用户自定义 OpenAI 兼容云端 Provider（经本地 cloud_proxy 按 slug 路由）。

    不硬编码任何提供商 URL，仅通过 ``provider_slug`` 告知 Rust 代理应当查哪条
    ``cloud_providers`` 记录取 base_url；所有请求/流式逻辑复用
    :class:`OpenAIProvider`，保证云端 Prompt 变体与错误捕获一致。
    """

    version: PromptVersion = "cloud"

    def __init__(
        self,
        provider_slug: str | None = None,
        *,
        base_url: str | None = None,
        model: str = _DEFAULT_CLOUD_MODEL,
        token: str | None = None,
        timeout: float = CLOUD_TIMEOUT_S,
        max_retries: int = 1,
        default_max_tokens: int | None = None,
    ) -> None:
        """初始化通用云端 Provider。

        Args:
            provider_slug: 提供商标识（DB ``cloud_providers.provider_key``）；
                ``None`` 时读取 env ``FILEMIND_ACTIVE_CLOUD_PROVIDER``，再为空
                回落 ``openai``（与老版本兼容，避免首次升级空配置即崩）。
            base_url: 显式覆盖代理端点（仅单测用，生产留空即可，由
                ``provider_slug`` 自动推导 ``/cloud-proxy/{slug}``）。
            model: 生成模型名（默认 ``FILEMIND_CLOUD_MODEL`` env）。
            token: 代理共享 token；``None`` 时回落 ``FILEMIND_CLOUD_PROXY_TOKEN``。
            timeout: 单次调用超时（秒）。
            max_retries: SDK 重试次数。
            default_max_tokens: provider 级默认输出上限；调用方未显式指定
                ``max_tokens`` 时生效；``None`` 表示沿用后端默认。
        """
        slug = provider_slug or os.environ.get(_ACTIVE_PROVIDER_ENV, "openai")
        effective_base = base_url if base_url is not None else _default_proxy_endpoint(slug)
        super().__init__(
            base_url=effective_base,
            model=model,
            token=token,
            timeout=timeout,
            max_retries=max_retries,
            default_max_tokens=default_max_tokens,
        )

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """自定义云端 Embedding 未启用。

        与 OpenAIProvider 语义一致：云端 Embedding 路由未在 Rust 代理暴露；
        检索统一使用本地 bge。此处报错保持调用链语义诚实。

        Args:
            texts: 待向量化文本列表（顺序保留）。
            **kwargs: 保留透传参数（本实现忽略）。

        Returns:
            永不返回，始终抛异常。

        Raises:
            CloudUnavailableError: 始终（云端 Embedding 未启用）。
        """
        raise CloudUnavailableError(
            "自定义云端 Embedding 未启用：本地代理不暴露 embeddings 路由，检索固定使用本地 bge"
        )
