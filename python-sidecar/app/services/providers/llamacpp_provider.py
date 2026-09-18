"""T3b — LlamaCppProvider：Sidecar 内置 llama.cpp 引擎（llama-server）的 Provider 实现。

用途：未安装 Ollama 的部署机器上完成生成（RAG 回答 / 查询改写 / 分类 / 自我纠正），
使「本地模式」不再隐含「必须装 Ollama」。引擎进程由
:mod:`app.services.local_llm_service` 托管（随包分发的 llama-server 子进程）。

为什么直接写 httpx 而不用 openai SDK：llama-server 只暴露 OpenAI 兼容的最小子集，
本 Provider 需要的正是那几条（`/v1/chat/completions`，``stream`` 与
``response_format``）；用 SDK 会额外引入一层超时/重试语义，而本地 CPU 推理的
「模型冷加载」耗时特征与云端差异很大，需要自己掌控超时预算。

错误映射（与 OllamaProvider 对齐，调用方无需区分后端）：

- 引擎不可用（未开启内置后端 / 产物缺失 / 权重未下载 / 启动失败）
  → :class:`LLMUnavailableError`
- HTTP / 超时失败 → 同上

能力边界：``embed`` 与 OllamaProvider 一样显式不支持——向量化统一走进程内 ONNX，
同一份向量不能混用不同后端。
"""

from __future__ import annotations

import asyncio
import json
import os
from typing import TYPE_CHECKING

import httpx

from app.rules.llm_classify import LLMUnavailableError
from app.services import local_llm_service
from app.services.cloud_provider import LLMProvider
from app.services.embedding_service import EmbeddingUnavailableError
from app.services.model_specs import LLM_MODEL_NAME

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    from app.services.cloud_provider import PromptVersion

#: 非流式补全超时（秒）。CPU 推理明显慢于 Ollama+GPU：256 token 的 JSON 输出在
#: 低配机上可能数十秒，故默认给到 120s（Ollama 侧只要 30s）。
REQUEST_TIMEOUT = float(os.environ.get("FILEMIND_LLAMA_REQUEST_TIMEOUT", "120") or "120")
#: 流式生成总量超时（秒）。与 ``generation_service.LOCAL_STREAM_TIMEOUT`` 同口径
#: （180s）：本地流式一旦开跑必须跑完，中途掐断会丢后半段回答。
STREAM_TIMEOUT = float(os.environ.get("FILEMIND_LLAMA_STREAM_TIMEOUT", "180") or "180")
#: SSE 数据行前缀 / 结束标记（llama-server 与 OpenAI 一致）
_SSE_DATA_PREFIX = "data: "
_SSE_DONE = "[DONE]"
#: 补全路径（OpenAI 兼容）
_COMPLETIONS_PATH = "/v1/chat/completions"


def build_payload(
    system: str,
    user: str,
    temperature: float,
    max_tokens: int | None,
    *,
    stream: bool,
    json_mode: bool = False,
) -> dict[str, object]:
    """构造 Chat Completions 请求体（纯函数，便于单测）。

    Args:
        system: system 提示词。
        user: user 提示词。
        temperature: 采样温度。
        max_tokens: 最大输出 token 数；``None`` 时不下发（用引擎默认）。
        stream: 是否流式。
        json_mode: 是否约束 JSON 输出（P-01/P-02 依赖，实测引擎支持）。

    Returns:
        请求体字典；仅包含引擎实际接受的字段。
    """
    payload: dict[str, object] = {
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": temperature,
        "stream": stream,
    }
    if max_tokens is not None:
        payload["max_tokens"] = max_tokens
    if json_mode:
        payload["response_format"] = {"type": "json_object"}
    return payload


def parse_content(body: object) -> str:
    """从非流式响应体取首个 choice 的正文（结构异常返回空串）。

    结构异常不抛异常：调用方的 JSON 解析失败/空结果都有既有降级路径
    （P-02 原样返回原查询、P-01 跳过第 3 层），返回空串即可自然落入其中。
    """
    if not isinstance(body, dict):
        return ""
    choices = body.get("choices")
    if not isinstance(choices, list) or not choices:
        return ""
    first = choices[0]
    if not isinstance(first, dict):
        return ""
    message = first.get("message")
    if not isinstance(message, dict):
        return ""
    content = message.get("content")
    return content if isinstance(content, str) else ""


def delta_of_sse_line(line: str) -> str:
    """从单行 SSE 取出 delta 正文（非 data 行 / ``[DONE]`` / 空 delta → 空串）。"""
    if not line.startswith(_SSE_DATA_PREFIX):
        return ""
    data = line[len(_SSE_DATA_PREFIX) :].strip()
    if not data or data == _SSE_DONE:
        return ""
    try:
        chunk = json.loads(data)
    except json.JSONDecodeError:
        return ""
    if not isinstance(chunk, dict):
        return ""
    choices = chunk.get("choices")
    if not isinstance(choices, list) or not choices:
        return ""
    first = choices[0]
    if not isinstance(first, dict):
        return ""
    delta = first.get("delta")
    if not isinstance(delta, dict):
        return ""
    content = delta.get("content")
    return content if isinstance(content, str) else ""


class LlamaCppProvider(LLMProvider):
    """内置 llama.cpp 引擎的 Provider（Sidecar 子进程 + OpenAI 兼容 HTTP）。

    ``version = "local"``：与 Ollama 同样走本地 prompt 变体（详细指令 + 多 few-shot），
    云端的精简变体不适用。
    """

    version: PromptVersion = "local"

    def __init__(self, model: str = LLM_MODEL_NAME) -> None:
        """初始化。

        Args:
            model: GGUF 模型标识（对应 ``model_specs`` 注册的本地生成模型）。
        """
        self._model = model

    async def _base_url(self) -> str:
        """确保引擎在运行并返回 base URL。

        Raises:
            LLMUnavailableError: 内置后端未开启 / 引擎产物缺失 / 权重未下载 / 启动失败。
        """
        try:
            return await local_llm_service.ensure_server(self._model)
        except local_llm_service.LocalLlmUnavailableError as exc:
            raise LLMUnavailableError(f"内置生成引擎不可用: {exc}") from exc

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
        """非流式补全（分类 / 改写 / 自我纠正的 JSON 输出走这里）。

        Raises:
            LLMUnavailableError: 引擎不可用，或请求失败 / 超时。
        """
        base_url = await self._base_url()
        payload = build_payload(
            system, user, temperature, max_tokens, stream=False, json_mode=json_mode
        )
        try:
            async with httpx.AsyncClient(timeout=REQUEST_TIMEOUT) as client:
                resp = await client.post(
                    f"{base_url}{_COMPLETIONS_PATH}",
                    json=payload,
                    headers=local_llm_service.auth_headers(),
                )
                resp.raise_for_status()
                body: object = resp.json()
        except (httpx.HTTPError, ValueError) as exc:
            raise LLMUnavailableError(f"内置生成引擎调用失败: {exc}") from exc
        return parse_content(body)

    async def generate_stream(
        self,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        **kwargs: object,
    ) -> AsyncIterator[str]:
        """流式补全，逐 delta yield（RAG 生成的 token 流）。

        Yields:
            非空内容 delta。

        Raises:
            LLMUnavailableError: 引擎不可用，或首个 delta 前后请求失败 / 超时。
        """
        base_url = await self._base_url()
        payload = build_payload(system, user, temperature, max_tokens, stream=True)
        try:
            async with (
                httpx.AsyncClient(timeout=REQUEST_TIMEOUT) as client,
                client.stream(
                    "POST",
                    f"{base_url}{_COMPLETIONS_PATH}",
                    json=payload,
                    headers=local_llm_service.auth_headers(),
                ) as resp,
            ):
                resp.raise_for_status()
                async with asyncio.timeout(STREAM_TIMEOUT):
                    async for line in resp.aiter_lines():
                        delta = delta_of_sse_line(line)
                        if delta:
                            yield delta
        except TimeoutError as exc:
            # 总量超时：本地引擎卡死时防后台流无限挂起（同 LOCAL_STREAM_TIMEOUT 的取舍）
            raise LLMUnavailableError(f"内置生成引擎流式超时（>{STREAM_TIMEOUT}s）") from exc
        except httpx.HTTPError as exc:
            raise LLMUnavailableError(f"内置生成引擎流式生成失败: {exc}") from exc

    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """显式不支持：向量化统一为 Sidecar 进程内 ONNX 推理。

        Raises:
            EmbeddingUnavailableError: 恒定抛出，指引改用 ``embedding_service``。
        """
        raise EmbeddingUnavailableError(
            "LlamaCppProvider 不提供向量化：请在 app.services.embedding_service "
            "中调用 embed_texts/embed_text（进程内 ONNX 推理）"
        )
