"""T8.4 — DeepSeekProvider：OpenAI 兼容格式 + 中文优化参数。

来源：08_Prompt工程设计 §6 / T8.4 任务标题。

DeepSeek API 与 OpenAI Chat Completions 格式兼容（消息结构、temperature、
max_tokens、``response_format=json_object`` 均一致），因此直接复用
:class:`~app.services.providers.openai_provider.OpenAIProvider` 的全部请求 /
流式 / 错误映射逻辑，仅差异：
- base_url 指向本地代理 deepseek 端点（``/cloud-proxy/deepseek``）
- 默认模型 ``deepseek-chat``（env ``FILEMIND_DEEPSEEK_MODEL`` 可覆盖）
- 中文优化参数：默认 ``max_tokens=2048``（经父类 ``default_max_tokens``，
  中文 token 密度低于英文，给足输出上限防长回答截断；调用方可显式传
  ``None`` 恢复后端默认）
- ``embed`` 明确报错：DeepSeek 官方不提供 Embedding API
"""

from __future__ import annotations

import os

from app.services.cloud_provider import CloudUnavailableError
from app.services.providers.openai_provider import (
    CLOUD_PROXY_BASE_URL,
    CLOUD_TIMEOUT_S,
    OpenAIProvider,
)

#: DeepSeek 生成模型名（env 可覆盖；deepseek-chat 原生优化中文）
DEEPSEEK_MODEL = os.environ.get("FILEMIND_DEEPSEEK_MODEL", "deepseek-chat")
#: 中文优化：默认输出上限（token）；调用方显式传 ``None`` 时恢复后端默认
_DEFAULT_MAX_TOKENS = 2048


class DeepSeekProvider(OpenAIProvider):
    """DeepSeek 云端推理 Provider（OpenAI 兼容格式，经本地 cloud_proxy）。

    继承 :class:`OpenAIProvider` 的 ``generate`` / ``generate_stream`` / 错误映射，
    仅差异化 base_url、默认模型、中文优化默认输出上限与 ``embed`` 语义。
    """

    def __init__(
        self,
        base_url: str = f"{CLOUD_PROXY_BASE_URL}/cloud-proxy/deepseek",
        model: str = DEEPSEEK_MODEL,
        token: str | None = None,
        timeout: float = CLOUD_TIMEOUT_S,
        max_retries: int = 1,
    ) -> None:
        """初始化。

        Args:
            base_url: 本地代理的 OpenAI 兼容端点（DeepSeek 走 ``deepseek`` 段）。
            model: DeepSeek 模型名（默认 ``deepseek-chat``）。
            token: 代理共享 token；``None`` 时取 ``FILEMIND_CLOUD_PROXY_TOKEN``。
            timeout: 单次调用超时（秒）。
            max_retries: SDK 重试次数。
        """
        super().__init__(
            base_url=base_url,
            model=model,
            token=token,
            timeout=timeout,
            max_retries=max_retries,
            default_max_tokens=_DEFAULT_MAX_TOKENS,
        )

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """DeepSeek 无 Embedding 服务，明确报错。

        Raises:
            CloudUnavailableError: 始终（DeepSeek 未提供 Embedding API）。
        """
        raise CloudUnavailableError("DeepSeek 未提供 Embedding API，检索固定使用本地 bge")
