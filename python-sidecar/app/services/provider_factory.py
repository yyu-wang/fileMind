"""T8.5 — Provider 工厂：模型名 → Provider 解析 + 上下文窗口 / Prompt 版本适配。

来源：08_Prompt工程设计 §6（适配策略）。

职责（供调用点接线使用）：
- :func:`resolve_provider`：按模型名解析对应 Provider 实例（云端前缀走云端
  类，其余回落本地 Ollama）
- ``MODEL_CONTEXT_LIMITS`` / :func:`get_max_context` / :func:`truncate_context`：
  Token 长度适配——按模型上下文窗口截断检索上下文，预留生成空间
- :func:`prompt_version_for` / ``PromptVersion``：Prompt 指令强度版本
  （本地详细 + 多 Few-shot；云端精简 + 少 Few-shot，利用 ``response_format``
  强约束），DoD「同一请求在 local/cloud 下输出格式一致」的 prompt 侧依据

设计：每次 :func:`resolve_provider` 返回全新实例（无共享可变状态，规则 19），
Provider 仅持连接参数、不持缓存；请求级选择由调用点（路由层）完成。
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

from app.core.logging import getLogger
from app.rules.llm_classify import LLM_MODEL
from app.services import local_llm_service
from app.services.providers.deepseek_provider import DeepSeekProvider
from app.services.providers.generic_cloud_provider import GenericCloudProvider
from app.services.providers.llamacpp_provider import LlamaCppProvider
from app.services.providers.ollama_provider import OllamaProvider
from app.services.providers.openai_provider import OpenAIProvider

if TYPE_CHECKING:
    from app.services.cloud_provider import LLMProvider, PromptVersion

logger = getLogger("filemind.provider")

#: Rust 启动 Sidecar 时注入的激活云提供商 slug（对应 DB active_cloud_provider）
_ACTIVE_CLOUD_PROVIDER_ENV = "FILEMIND_ACTIVE_CLOUD_PROVIDER"

#: 本地生成「实际生效后端」的缓存（由探测服务回写；``None`` = 尚未判定）
#:
#: 为什么不在这里探测：:func:`resolve_provider` 是同步函数（调用点遍布路由与规则层），
#: 不能在其中发网络请求。判定的唯一事实来源是探测服务（启动时后台探一次 + 设置页
#: 「重新检测」），它同时知道 Ollama 可用性与内置引擎前置条件，回写结果供此处只读。
_effective_local_backend: str | None = None

#: 生成模型名 → 上下文窗口（token）；未收录模型回落保守本地值
#: （08-§6 模型差异矩阵：本地 32K / gpt-4o 128K / deepseek-chat 64K）
MODEL_CONTEXT_LIMITS: dict[str, int] = {
    "qwen3.8-27b": 32000,
    "gpt-4o": 128000,
    "deepseek-chat": 64000,
}
#: 未知模型上下文窗口回落值（保守取本地模型）
DEFAULT_CONTEXT_LIMIT = 32000
#: 生成预留 token 数（对齐 08-§6 get_max_context 预留 2K）
GENERATION_RESERVE = 2000

#: 云端模型名前缀 → Provider 类。命中任一前缀即按云端处理（resolve 与
#: prompt_version 共用，保证「选择」与「适配」判定一致）。
#: 类型用具体类并集而非 ``type[LLMProvider]``：mypy 对抽象基类构造器不识别
#: ``model`` 关键字（ABC 未声明 __init__）；并集中各实现均接受 ``model=``。
_CLOUD_ROUTES: tuple[tuple[str, type[OllamaProvider] | type[OpenAIProvider]], ...] = (
    ("gpt-", OpenAIProvider),
    ("deepseek-", DeepSeekProvider),
)


def active_cloud_provider_slug() -> str | None:
    """返回 Rust 注入的激活云提供商 slug（去首尾空白，空串回落 ``None``）。

    Returns:
        激活 slug；env 未设置/空串返回 ``None``。
    """
    value = os.environ.get(_ACTIVE_CLOUD_PROVIDER_ENV, "")
    stripped = value.strip()
    return stripped if stripped else None


def record_local_backend(backend: str) -> None:
    """回写「本地生成实际该用哪个后端」（仅供探测服务调用）。

    Args:
        backend: ``ollama`` 或 ``builtin``（``local_llm_service`` 的取值）。
    """
    global _effective_local_backend
    if _effective_local_backend != backend:
        logger.info("provider.local_backend_resolved", backend=backend)
    _effective_local_backend = backend


def effective_local_backend() -> str:
    """当前生效的本地生成后端。

    尚未判定（探测未跑或未回写）时回落用户配置值——保持「默认走 Ollama」的既有
    行为，不会因为探测迟到就改变语义。
    """
    return _effective_local_backend or local_llm_service.configured_backend()


def reset_local_backend_cache() -> None:
    """清空生效后端缓存（仅测试用）。"""
    global _effective_local_backend
    _effective_local_backend = None


def get_max_context(model: str) -> int:
    """按模型上下文窗口返回可用上限（扣 2K 生成预留）。

    Args:
        model: 生成模型名。

    Returns:
        输入上下文允许的最大 token 数；未收录模型回落 ``DEFAULT_CONTEXT_LIMIT``。
    """
    limit = MODEL_CONTEXT_LIMITS.get(model, DEFAULT_CONTEXT_LIMIT)
    return limit - GENERATION_RESERVE


def truncate_context(context: str, model: str) -> str:
    """按模型上下文窗口截断检索上下文（保留前缀，尾部标记）。

    Args:
        context: 待截断的检索上下文文本。
        model: 生成模型名。

    Returns:
        未超窗返回原样；超窗保留前 ``get_max_context(model)`` 字符并追加截断标记。
    """
    # SC-m7：get_max_context 返回 token 数，按 ~4 chars/token 近似换算为字符数
    # （英文偏保守多截，中文偏激进少截，宁可多截不漏截）
    max_chars = get_max_context(model) * 4
    if len(context) <= max_chars:
        return context
    return context[:max_chars] + "\n[上下文已截断]"


def resolve_provider(model: str = LLM_MODEL) -> LLMProvider:
    """按模型名解析 Provider 实例。

    判定顺序（T3b 改造后）：
    1. 命中内置前缀 ``gpt-`` / ``deepseek-`` → 分别用 OpenAIProvider / DeepSeekProvider
    2. 存在激活云提供商 slug（env ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 非空）
       → 使用 :class:`GenericCloudProvider`，按 slug 代理路由
    3. 其余为本地生成，按生效后端分流：
       - ``builtin``（用户显式选择，或探测发现 Ollama 不可用而自动回落）
         → :class:`LlamaCppProvider`（Sidecar 内置 llama.cpp 引擎）
       - 其余 → :class:`OllamaProvider`（既有默认行为）

    Args:
        model: 生成模型名（默认 ``LLM_MODEL``）。

    Returns:
        对应 Provider 的**全新实例**（每次调用独立，无共享状态）。
    """
    for prefix, provider_cls in _CLOUD_ROUTES:
        if model.startswith(prefix):
            return provider_cls(model=model)
    if active_cloud_provider_slug() is not None:
        return GenericCloudProvider(model=model)
    if effective_local_backend() == local_llm_service.BACKEND_BUILTIN:
        return LlamaCppProvider()
    return OllamaProvider(model=model)


def resolve_local_provider(model: str) -> LLMProvider | None:
    """解析**本地**生成 Provider（不参与云端判定）。

    与 :func:`resolve_provider` 的分工：调用方（路由层）已按推理模式过滤过云端，这里若
    复用 :func:`resolve_provider` 会把 ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 重新带回来
    （切回本地后 env 冻结 → 云端代理 → 缺 Key 401，见 ``routes_chat._resolve_chat_provider``
    的注释）。

    生效后端为内置引擎（T3：用户显式选 builtin，或探测发现 Ollama 不可用而自动回落）时
    返回 :class:`LlamaCppProvider`；否则返回 ``None``，含义是「走调用点既有的 Ollama
    路径」——用 ``None`` 而不是 ``OllamaProvider`` 是为了让默认路径**零行为变化**。

    Args:
        model: 生成模型名（本地模型名；内置引擎忽略它，用配置的 GGUF 标识）。

    Returns:
        内置引擎 Provider；默认（Ollama）后端返回 ``None``。
    """
    if effective_local_backend() == local_llm_service.BACKEND_BUILTIN:
        return LlamaCppProvider()
    return None


def resolve_cloud_provider(model: str) -> LLMProvider | None:
    """解析云端 Provider；本地模型 / 空串返回 ``None``。

    供路由层选择推理后端（T8.5）：``None`` 表示回落调用点各自的既有本地路径
    （``call_ollama_json`` / ``AsyncClient``），行为与引入 Provider 前一致；
    仅云端模型走 Provider 统一路径。判定与 :func:`resolve_provider` 同源，
    避免「选择」与「适配」两处规则漂移。

    Args:
        model: 生成模型名（空串表示未指定，回落本地）。

    Returns:
        云端 Provider 实例；本地/空串模型返回 ``None``。
    """
    if not model:
        return None
    provider = resolve_provider(model)
    return provider if provider.version == "cloud" else None


def prompt_version_for(model: str) -> PromptVersion:
    """按模型名判定 Prompt 版本（local / cloud）。

    P-07 改造后：当 env ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 存在时，任何模型名
    （内置前缀除外，已经判定为云端）也按云端处理，保证 GenericCloudProvider
    选到正确的 prompt 变体。

    Args:
        model: 生成模型名。

    Returns:
        ``"cloud"``：云端模型（利用 ``response_format`` 强约束，prompt 精简）；
        ``"local"``：本地模型（prompt 需更强格式约束与更多 Few-shot）。
    """
    for prefix, _ in _CLOUD_ROUTES:
        if model.startswith(prefix):
            return "cloud"
    if active_cloud_provider_slug() is not None:
        return "cloud"
    return "local"
