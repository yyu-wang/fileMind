"""T3 — 内置本地生成引擎：Sidecar 进程内托管的 ``llama-server`` 子进程。

背景：本地生成此前只有 Ollama 一条路（``OllamaProvider`` → ``127.0.0.1:11434``）。
未安装 Ollama 的部署机器上，即使 Embedding / Rerank / GGUF 都就绪，回答仍生不出来。
本模块把 llama.cpp 的 ``llama-server``（随安装包分发的预编译二进制）作为 Sidecar 的
**子进程**拉起，对外提供 OpenAI 兼容 HTTP 接口，实现「不装 Ollama 也能知识问答」。

本模块只负责**运行态与生命周期编排**（谁在跑、跑在哪个端口、当前是哪个模型、
空闲是否该卸载）；产物定位与子进程操作等无状态原语在
:mod:`app.services.local_llm_engine`（含「为什么用子进程而不是 llama-cpp-python」）。

启动条件（全满足才拉起）：后端开关为 ``builtin``（env ``FILEMIND_LOCAL_LLM_BACKEND``，
Rust 从 app_config 注入）且 GGUF 权重已下载到本机。任一不满足 → 抛
:class:`LocalLlmUnavailableError`，由调用方回落（Ollama 可用则用 Ollama）。

生命周期要点：

- **端口**：每次启动现取空闲端口（避免与用户自己的 8080/11434 冲突；孤儿进程占用的
  旧端口也不会影响新建实例）；
- **就绪**：轮询 ``/health``，期间同步探子进程是否已退出（加载失败会秒退，
  早失败优于等满超时）；失败时把子进程 stderr 尾部带进异常，便于定位；
- **空闲卸载**：下次取用时惰性检查，空闲超过阈值即停掉子进程释放内存（不引定时器）；
- **不留孤儿**：正常退出经 ``atexit`` 停止；被强杀（SIGKILL）时下一轮启动按 PID 文件清理。
"""

from __future__ import annotations

import asyncio
import atexit
import os
import secrets
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.core.logging import getLogger
from app.services import local_llm_engine, model_download_service

# 显式再导出：调用方（provider / probe）按「引擎不可用」这一概念捕获本异常，
# 而异常的归属模块是 engine（无状态层），故在此按 PEP 484 的显式再导出写法暴露。
from app.services.local_llm_engine import LocalLlmUnavailableError as LocalLlmUnavailableError
from app.services.model_specs import LLM_MODEL_NAME, llm_gguf_path

if TYPE_CHECKING:
    import subprocess
    from pathlib import Path

logger = getLogger("filemind.local_llm")

#: 后端开关 env（Rust 从 app_config.local_llm_backend 注入）
_ENV_BACKEND = "FILEMIND_LOCAL_LLM_BACKEND"
#: 内置 GGUF 模型标识 env（Rust 从 app_config.local_llm_model 注入）
_ENV_MODEL = "FILEMIND_LOCAL_LLM_MODEL"

#: 内置后端开关取值
BACKEND_BUILTIN = "builtin"

#: 空闲卸载阈值（秒，env 可覆盖；默认 600s = 10min 无推理即停掉子进程释放内存）
IDLE_UNLOAD = float(os.environ.get("FILEMIND_LLAMA_IDLE_UNLOAD", "600") or "600")


@dataclass(frozen=True)
class LocalLlmStatus:
    """内置引擎运行态快照（供日志、健康检查与设置页展示）。"""

    running: bool
    """子进程是否在运行。"""

    base_url: str
    """运行中的引擎 base URL（未运行时空串）。"""

    model: str
    """当前加载的 GGUF 模型标识（未运行时空串）。"""

    pid: int | None
    """子进程 PID（未运行时 ``None``）。"""

    last_error: str
    """最近一次启动失败原因（成功启动后清空）。"""


@dataclass
class _ServerState:
    """当前引擎进程状态（进程内可变）。"""

    process: subprocess.Popen[bytes] | None = None
    """引擎子进程句柄（未运行时 ``None``）。"""

    base_url: str = ""
    """运行中的引擎 base URL。"""

    model: str = ""
    """当前加载的 GGUF 模型标识。"""

    api_key: str = ""
    """本次运行的一次性 API Key（每次启动重新生成；仅经 env 交给引擎，不进 argv）。"""

    last_used: float = 0.0
    """最近一次被取用的 monotonic 时间（空闲卸载判定）。"""

    last_error: str = ""
    """启动失败原因（供探测/日志展示；成功启动后清空）。"""


_state = _ServerState()
#: 启动/停止的并发保护（避免并发请求各起一个引擎）
_lifecycle_lock = asyncio.Lock()


def reset_state() -> None:
    """清空状态（仅测试用；不负责停进程）。"""
    global _state
    _state = _ServerState()


def _env(name: str, default: str = "") -> str:
    """读取 env（去首尾空白）。"""
    return os.environ.get(name, "").strip() or default


def configured_backend() -> str:
    """当前配置的本地生成后端（``ollama`` / ``builtin``）。"""
    return _env(_ENV_BACKEND, "ollama").lower()


def configured_model() -> str:
    """当前配置的内置 GGUF 模型标识。"""
    return _env(_ENV_MODEL, LLM_MODEL_NAME)


def _process() -> object | None:
    """当前子进程句柄（未运行时 ``None``）。"""
    return _state.process


def _is_usable() -> bool:
    """当前子进程是否仍在运行。"""
    proc = _state.process
    return proc is not None and proc.poll() is None


def status() -> LocalLlmStatus:
    """返回内置引擎运行态快照（不触发任何启动/停止）。"""
    running = _is_usable()
    proc = _state.process
    return LocalLlmStatus(
        running=running,
        base_url=_state.base_url if running else "",
        model=_state.model if running else "",
        pid=proc.pid if running and proc is not None else None,
        last_error=_state.last_error,
    )


def auth_headers() -> dict[str, str]:
    """调用引擎所需的鉴权头（引擎未运行时返回空 dict）。

    为什么需要：引擎监听随机本地端口，**不加鉴权时本机任何进程都能拿它做推理**
    （白占 CPU 与约 2GB 内存），也能读走我们发给它的提示词。这与 Sidecar 自身
    「本地也要 PSK 握手」的取舍一致。Key 每次启动重新生成，不落盘、不进 argv、
    不经 :func:`status` 暴露。
    """
    if _is_usable() and _state.api_key:
        return {"Authorization": f"Bearer {_state.api_key}"}
    return {}


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


def stop_server() -> None:
    """停止引擎子进程（幂等；供 shutdown 与 atexit 调用）。"""
    local_llm_engine.stop_process(_state.process)
    _state.process = None
    _state.base_url = ""
    _state.model = ""
    # 一并丢弃 API Key：下次启动重新生成，旧 key 不再有效
    _state.api_key = ""


def _unload_if_idle() -> None:
    """空闲超过阈值 → 停掉引擎释放内存（与 rerank 同款惰性检查，不引定时器）。"""
    if _is_usable() and time.monotonic() - _state.last_used >= IDLE_UNLOAD:
        logger.info("local_llm.idle_unload", idle_s=round(time.monotonic() - _state.last_used))
        stop_server()


async def _start(model: str) -> str:
    """拉起引擎并等待就绪，返回 base URL。

    Raises:
        LocalLlmUnavailableError: 二进制缺失 / 权重未下载 / 启动失败或超时。
    """
    binary = local_llm_engine.server_binary_path()
    if binary is None or not binary.is_file():
        raise LocalLlmUnavailableError(
            "内置生成引擎可执行文件缺失（未随包分发或未执行 "
            "scripts/fetch-llama-server.sh）；请改用 Ollama 或重新安装应用"
        )
    if not os.access(binary, os.X_OK):
        raise LocalLlmUnavailableError(f"内置生成引擎不可执行（缺少执行权限）: {binary.name}")
    gguf = _gguf_for(model)
    local_llm_engine.kill_orphan_from_pid_file()
    port = local_llm_engine.pick_free_port()
    base_url = f"http://127.0.0.1:{port}"
    started = time.monotonic()
    # 一次性 API Key：只经 env 交给引擎，进程内保存供 auth_headers() 复用
    api_key = secrets.token_urlsafe(32)
    proc = local_llm_engine.spawn(local_llm_engine.build_command(binary, gguf, port), api_key)
    _state.process = proc
    _state.api_key = api_key
    try:
        # 这里的状态已是本次启动的，auth_headers() 给出的正是本次 key 的头
        await local_llm_engine.wait_ready(proc, base_url, model, auth_headers())
    except LocalLlmUnavailableError as exc:
        local_llm_engine.stop_process(proc)
        _state.process = None
        _state.api_key = ""
        # 留下失败原因：设置页/日志据此展示「引擎为何起不来」，而不是只看到请求失败
        _state.last_error = str(exc)
        raise
    local_llm_engine.record_pid(proc.pid)
    _state.base_url = base_url
    _state.model = model
    _state.last_used = time.monotonic()
    _state.last_error = ""
    logger.info(
        "local_llm.started",
        model=model,
        port=port,
        ms=int((time.monotonic() - started) * 1000),
    )
    return base_url


async def ensure_server(model: str | None = None) -> str:
    """确保内置引擎在运行，返回其 base URL。

    幂等：已在运行且模型一致时直接返回（并刷新使用时间）；换模型 / 进程已退出时
    重启。并发调用由生命周期锁串行化，不会拉起多个引擎。

    Args:
        model: GGUF 模型标识；``None`` 用配置值（env → 默认模型）。

    Returns:
        引擎 base URL（如 ``http://127.0.0.1:53211``）。

    Raises:
        LocalLlmUnavailableError: 后端未开启、二进制缺失、权重未下载或启动失败。
    """
    target = model or configured_model()
    if configured_backend() != BACKEND_BUILTIN:
        raise LocalLlmUnavailableError(
            f"内置生成后端未开启（当前 {configured_backend()}）；"
            "请在「设置」中切换为内置引擎，或改用 Ollama"
        )
    async with _lifecycle_lock:
        _unload_if_idle()
        if _is_usable() and _state.model == target:
            _state.last_used = time.monotonic()
            return _state.base_url
        # 换模型 / 进程已退出：先停干净再起，避免两个引擎同时占内存
        stop_server()
        return await _start(target)


async def engine_ready() -> tuple[bool, str]:
    """探测内置引擎可用性（不启动进程，仅检查前置条件）。

    Returns:
        ``(是否可用, 说明)``：可用于设置页/健康检查展示；说明为空表示无异常。
    """
    if configured_backend() != BACKEND_BUILTIN:
        return False, f"后端为 {configured_backend()}"
    binary = local_llm_engine.server_binary_path()
    if binary is None or not binary.is_file():
        return False, "引擎可执行文件缺失"
    model = configured_model()
    if not model_download_service.model_ready(model):
        return False, f"权重未下载（{model}）"
    return True, ""


atexit.register(stop_server)
