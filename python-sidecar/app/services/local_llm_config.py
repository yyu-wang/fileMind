"""T3 — 内置生成引擎的配置读取、权重定位与前置条件判定。

（2026-09-18 按职责拆自 ``local_llm_service.py``，313 行超 Python 警告阈值 300；
运行态（谁在跑 / 端口 / 当前模型 / 空闲卸载）与生命周期编排仍在
:mod:`app.services.local_llm_service`。）

本模块只回答「该用哪个后端与模型、二进制与权重在不在」，**不启动任何进程**。
调用方除编排层外还有探测回落判定（``inference_probe_service``）与 Provider 选择
（``provider_factory``），它们只用这里的配置与前置条件查询。
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

from app.services import local_llm_engine, model_download_service
from app.services.local_llm_engine import LocalLlmUnavailableError
from app.services.model_specs import LLM_MODEL_NAME, llm_gguf_path

if TYPE_CHECKING:
    from pathlib import Path

#: 后端开关 env（Rust 从 app_config.local_llm_backend 注入）
_ENV_BACKEND = "FILEMIND_LOCAL_LLM_BACKEND"
#: 内置 GGUF 模型标识 env（Rust 从 app_config.local_llm_model 注入）
_ENV_MODEL = "FILEMIND_LOCAL_LLM_MODEL"

#: 内置后端开关取值
BACKEND_BUILTIN = "builtin"


def _env(name: str, default: str = "") -> str:
    """读取 env（去首尾空白）。"""
    return os.environ.get(name, "").strip() or default


def configured_backend() -> str:
    """当前配置的本地生成后端（``ollama`` / ``builtin``）。"""
    return _env(_ENV_BACKEND, "ollama").lower()


def configured_model() -> str:
    """当前配置的内置 GGUF 模型标识。"""
    return _env(_ENV_MODEL, LLM_MODEL_NAME)


def _gguf_for(model: str) -> Path:
    """校验模型已注册且权重已下载，返回 GGUF 路径。

    Raises:
        LocalLlmUnavailableError: 模型未注册，或权重文件缺失。
    """
    try:
        gguf = llm_gguf_path(model)
    except ValueError as exc:
        raise LocalLlmUnavailableError(str(exc)) from exc
    if not gguf.is_file():
        raise LocalLlmUnavailableError(
            f"内置生成模型权重未下载: {model}（缺少 {gguf.name}）；"
            "请在「设置 → 本地生成模型（GGUF）」下载或导入"
        )
    return gguf


def engine_prerequisites() -> tuple[bool, str]:
    """内置引擎的**前置条件**是否齐备（不启动进程，也**不看后端配置**）。

    Returns:
        ``(是否齐备, 说明)``：说明为空表示前置齐备、可直接拉起引擎。

    为什么这里不判断「后端是否配置为 builtin」（原实现按配置短路，是 T3b 的真缺陷）：
    本函数的调用方就是探测的**回落判定**——「配置为 ollama 但 Ollama 不可用」正是需要
    它的场景。按配置短路会让回落永不发生，未安装 Ollama 的机器上默认配置依然问答不了，
    与 T3 的目标直接相悖。mock 掉本函数的单测因此掩盖了该缺陷（见回归用例
    ``test_probe_falls_back_to_builtin_when_ollama_down``）。
    """
    binary = local_llm_engine.server_binary_path()
    if binary is None or not binary.is_file():
        return False, "引擎可执行文件缺失"
    model = configured_model()
    if not model_download_service.model_ready(model):
        return False, f"权重未下载（{model}）"
    return True, ""
