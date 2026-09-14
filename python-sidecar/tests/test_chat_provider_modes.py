"""推理模式 → Provider 决策（T8.5 回归）。

从 ``test_chat_stream.py`` 拆出：这组用例**不涉及 SSE 事件流**，只验证
``_resolve_chat_provider`` 在 local / cloud 两种模式下的 Provider 解析结果。
拆开的原因见 rules/complexity.md（测试文件行数阈值），顺带让「事件流」与
「Provider 选路」两个关注点各自独立。

回归动机：Rust 启动 Sidecar 时注入 ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 且不随
切回本地刷新（冻结值）。若判定只看 env，本地模式也会被解析成云端 Provider
（→ 云端代理 → 无 Key 401），故 local 必须强制回落 ``None``（本地 Ollama）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.api.routes_chat import _resolve_chat_provider  # noqa: E402
from app.models import ChatStreamRequest  # noqa: E402
from app.services.providers.deepseek_provider import DeepSeekProvider  # noqa: E402
from app.services.providers.generic_cloud_provider import GenericCloudProvider  # noqa: E402


def _chat_request(inference_mode: str, llm_model: str) -> ChatStreamRequest:
    """构造仅关心模式/模型字段的最小请求体。"""
    return ChatStreamRequest(
        query="回归测试",
        table_name="documents_bge-small-zh-v1.5_v1",
        inference_mode=inference_mode,
        llm_model=llm_model,
    )


def test_local_mode_returns_none_with_frozen_active_cloud_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """回归：Rust 启动 Sidecar 时注入激活 slug env，且不随切回本地刷新。

    ``FILEMIND_ACTIVE_CLOUD_PROVIDER`` 冻结时，若 provider 判定只看 env，
    本地模型也会被解析成云端 Provider（→ 云端代理 → 无 Key 401）。local
    模式必须强制回落 ``None``（本地 Ollama）。
    """
    monkeypatch.setenv("FILEMIND_ACTIVE_CLOUD_PROVIDER", "deepseek")
    provider = _resolve_chat_provider(_chat_request("local", "qwen2.5:7b"))
    assert provider is None


def test_local_mode_forced_local_even_for_cloud_model_name(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """local 模式携带云端前缀模型名（异常输入）也不得误走云端。"""
    monkeypatch.setenv("FILEMIND_ACTIVE_CLOUD_PROVIDER", "deepseek")
    provider = _resolve_chat_provider(_chat_request("local", "deepseek-chat"))
    assert provider is None


def test_cloud_mode_builtin_prefix_resolves_cloud_provider() -> None:
    """cloud 模式 + 内置云端前缀 → DeepSeekProvider（env 缺失也可解析）。"""
    provider = _resolve_chat_provider(_chat_request("cloud", "deepseek-chat"))
    assert provider is not None
    assert isinstance(provider, DeepSeekProvider)
    assert provider.version == "cloud"


def test_cloud_mode_env_slug_resolves_generic_cloud(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """cloud 模式 + 激活自定义提供商 slug + 非内置前缀模型名 → GenericCloudProvider。"""
    monkeypatch.setenv("FILEMIND_ACTIVE_CLOUD_PROVIDER", "deepseek")
    provider = _resolve_chat_provider(_chat_request("cloud", "custom-model-v1"))
    assert isinstance(provider, GenericCloudProvider)
    assert provider.version == "cloud"
