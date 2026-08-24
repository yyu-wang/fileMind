"""T8.1 — LLM Provider 抽象接口（本地 / 云端推理统一契约）。

来源：08_Prompt工程设计 §6（Provider 抽象层）。

统一 sidecar 全部 LLM 调用点（流式生成 / 非流式 JSON / 向量化）的能力契约，
供 T8.2（OllamaProvider）、T8.3（OpenAIProvider）、T8.4（DeepSeekProvider）
各自实现，T8.5（本地 / 云端 Prompt 适配）在其上做输出格式归一。

与 08-§6 文档示例的差异（以任务标题为准，理由见下）：
- 方法命名 ``generate / generate_stream / embed``（文档示例为
  ``complete / stream_complete(prompt: str)``）
- 入参采用 (system, user) 消息对而非单个 prompt——对齐现有调用点
  ``generation_service.stream_generate`` / ``llm_classify.call_ollama_json``
- 额外抽象 ``embed`` 能力——T8.3 云端 Embedding API 所需

失败语义：实现方在后端不可用（连接 / HTTP / 超时 / 模型缺失）时抛出调用方
可捕获的领域异常（现有调用点使用 ``LLMUnavailableError`` /
``EmbeddingUnavailableError``）。接口层不导入具体异常类型，避免反向依赖。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

#: 支持的推理后端名称（T8.2–T8.4 各实现一个）
ProviderName = Literal["ollama", "openai", "deepseek"]
#: Prompt 版本（T8.5）：本地需强约束 + 多 Few-shot；云端利用 response_format
#: 可精简。用于 DoD「同一请求在 local/cloud 下输出格式一致」的 prompt 侧选择。
PromptVersion = Literal["local", "cloud"]


class CloudUnavailableError(Exception):
    """云端推理后端不可用（代理不可达 / HTTP 错误 / 鉴权失败 / 超时）。

    OpenAI 与 DeepSeek（T8.4）共用；T8.5 接入调用点时统一捕获以跳过第 3 层。
    定义在接口模块而非 openai_provider，使 rules 层（P-01 分类）可安全引用
    而不产生 services→rules 反向依赖（ollama_provider 已 import rules）。
    """


class LLMProvider(ABC):
    """LLM 推理 Provider 抽象接口。

    三个抽象方法分别对应 sidecar 现有的三类 LLM 调用：
    - :meth:`generate`：非流式补全（classify / rewrite / self-correct 的 JSON 输出）
    - :meth:`generate_stream`：流式补全（RAG 生成 P-03 的逐 token 推送）
    - :meth:`embed`：批量向量化（Embedding 检索）

    类属性 :attr:`version`：本 Provider 适用的 Prompt 版本（默认 ``"local"``，
    云端实现覆盖为 ``"cloud"``），供调用点选择 prompt 变体。
    """

    #: Prompt 版本（T8.5）：云端实现（OpenAI/DeepSeek）覆盖为 ``"cloud"``
    version: PromptVersion = "local"

    @abstractmethod
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
        """非流式补全，返回完整文本。

        Args:
            system: system 提示词。
            user: user 提示词。
            temperature: 采样温度（默认 0.2，与 P-01/P-03 一致）。
            max_tokens: 最大输出 token 数；``None`` 表示后端默认。
            json_mode: 是否约束 JSON 输出（本地映射 ``format="json"``，云端映射
                ``response_format``；classify / rewrite / self-correct 使用）。
            **kwargs: 透传后端特定参数。

        Returns:
            完整补全文本（不含思考链 token）。

        Raises:
            后端不可用（连接 / HTTP / 超时 / 模型缺失）时抛调用方可捕获的领域异常。
        """
        raise NotImplementedError

    @abstractmethod
    async def generate_stream(
        self,
        system: str,
        user: str,
        *,
        temperature: float = 0.2,
        max_tokens: int | None = None,
        **kwargs: object,
    ) -> AsyncIterator[str]:
        """流式补全，逐 token / 块 yield 内容 delta。

        参数与 :meth:`generate` 一致（流式场景不使用 ``json_mode``，
        生成阶段为纯文本输出，JSON 约束仅用于非流式 classify / rewrite）。

        Yields:
            非空内容 delta（后端逐 token 推送）。

        Raises:
            首个 delta 前后端不可用时抛调用方可捕获的领域异常。
        """
        # 抽象 async 生成器的类型标记：body 含 yield，mypy 才会把调用点视为
        # AsyncIterator[str] 而非 Coroutine（对齐 generation_service.stream_generate）。
        # 基类不可实例化，此分支永不执行。
        yield ""  # pragma: no cover

    @abstractmethod
    async def embed(self, texts: list[str], **kwargs: object) -> list[list[float]]:
        """批量向量化，顺序与输入一致。

        Args:
            texts: 待向量化文本列表（可为空列表，直接返回空列表）。
            **kwargs: 透传后端特定参数（如模型名覆盖）。

        Returns:
            与 ``texts`` 等长的向量列表。

        Raises:
            向量化后端不可用时抛调用方可捕获的领域异常。
        """
        raise NotImplementedError
