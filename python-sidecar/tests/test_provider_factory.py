"""T8.5 — Provider 工厂单元测试：模型名解析、上下文窗口、截断、Prompt 版本。

不涉及真实后端：断言解析到的 Provider 类型 / 窗口数值 / 截断行为 /
版本判定，全部纯函数级。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.provider_factory import (  # noqa: E402
    GENERATION_RESERVE,
    MODEL_CONTEXT_LIMITS,
    get_max_context,
    prompt_version_for,
    resolve_cloud_provider,
    resolve_provider,
    truncate_context,
)
from app.services.providers.deepseek_provider import DeepSeekProvider  # noqa: E402
from app.services.providers.ollama_provider import OllamaProvider  # noqa: E402
from app.services.providers.openai_provider import OpenAIProvider  # noqa: E402


def test_model_context_limits_values() -> None:
    """上下文窗口表与 08-§6 模型差异矩阵一致（本地 32K / gpt-4o 128K / deepseek 64K）。"""
    assert MODEL_CONTEXT_LIMITS["qwen3.8-27b"] == 32000
    assert MODEL_CONTEXT_LIMITS["gpt-4o"] == 128000
    assert MODEL_CONTEXT_LIMITS["deepseek-chat"] == 64000


def test_get_max_context_known_model() -> None:
    """已知模型：窗口扣 2K 生成预留。"""
    assert get_max_context("gpt-4o") == 128000 - GENERATION_RESERVE
    assert get_max_context("deepseek-chat") == 64000 - GENERATION_RESERVE


def test_get_max_context_unknown_falls_back_local() -> None:
    """未知模型回落保守本地值（32K 扣预留）。"""
    assert get_max_context("some-unknown-model") == 32000 - GENERATION_RESERVE


def test_truncate_context_within_window_unchanged() -> None:
    """未超窗返回原样。"""
    short = "短上下文"
    assert truncate_context(short, "gpt-4o") == short


def test_truncate_context_over_window_keeps_prefix_and_marker() -> None:
    """超窗保留前缀 + 截断标记。"""
    max_chars = get_max_context("qwen3.8-27b")
    long = "x" * (max_chars + 1000)
    assert truncate_context(long, "qwen3.8-27b") == long[:max_chars] + "\n[上下文已截断]"


def test_resolve_provider_cloud_prefixes() -> None:
    """gpt-* → OpenAIProvider；deepseek-* → DeepSeekProvider。"""
    assert isinstance(resolve_provider("gpt-4o"), OpenAIProvider)
    assert isinstance(resolve_provider("deepseek-chat"), DeepSeekProvider)


def test_resolve_provider_local_falls_back_ollama() -> None:
    """非云端前缀回落 OllamaProvider。"""
    assert isinstance(resolve_provider("qwen3.8-27b"), OllamaProvider)
    assert isinstance(resolve_provider(""), OllamaProvider)


def test_resolve_cloud_provider_cloud_returns_instance() -> None:
    """云端模型 → 云端 Provider 实例（version == "cloud"）。"""
    assert resolve_cloud_provider("gpt-4o").version == "cloud"
    assert resolve_cloud_provider("deepseek-chat").version == "cloud"


def test_resolve_cloud_provider_local_or_empty_returns_none() -> None:
    """本地模型 / 空串 → None（回落既有本地路径）。"""
    assert resolve_cloud_provider("qwen3.8-27b") is None
    assert resolve_cloud_provider("") is None


def test_prompt_version_for() -> None:
    """版本判定：云端前缀 "cloud"，其余 "local"。"""
    assert prompt_version_for("gpt-4o") == "cloud"
    assert prompt_version_for("deepseek-chat") == "cloud"
    assert prompt_version_for("qwen3.8-27b") == "local"
    assert prompt_version_for("") == "local"
