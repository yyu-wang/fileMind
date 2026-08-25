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

from typing import TYPE_CHECKING

from app.rules.llm_classify import LLM_MODEL
from app.services.providers.deepseek_provider import DeepSeekProvider
from app.services.providers.ollama_provider import OllamaProvider
from app.services.providers.openai_provider import OpenAIProvider

if TYPE_CHECKING:
    from app.services.cloud_provider import LLMProvider, PromptVersion

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
    """按模型名解析 Provider 实例（云端前缀走云端类，其余回落本地 Ollama）。

    Args:
        model: 生成模型名（默认 ``LLM_MODEL``）。

    Returns:
        对应 Provider 的**全新实例**（每次调用独立，无共享状态）。
    """
    for prefix, provider_cls in _CLOUD_ROUTES:
        if model.startswith(prefix):
            return provider_cls(model=model)
    return OllamaProvider(model=model)


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

    Args:
        model: 生成模型名。

    Returns:
        ``"cloud"``：云端模型（利用 ``response_format`` 强约束，prompt 精简）；
        ``"local"``：本地模型（prompt 需更强格式约束与更多 Few-shot）。
    """
    for prefix, _ in _CLOUD_ROUTES:
        if model.startswith(prefix):
            return "cloud"
    return "local"
