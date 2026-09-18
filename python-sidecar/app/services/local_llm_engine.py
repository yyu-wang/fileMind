"""T3 — 内置生成引擎的产物定位与子进程原语（**无运行态**）。

本模块是 :mod:`app.services.local_llm_service` 的底层：只回答「引擎可执行文件在哪、
启动参数怎么拼、子进程怎么起/怎么停」，不保存任何运行态（谁在跑、跑在哪个端口、
当前模型是什么，由 service 层持有）。

为什么用子进程而非进程内（llama-cpp-python）：内存门控按 Sidecar 进程 RSS 判定，
而 Embedding 的 ONNX 权重峰值已约 1.4GB，GGUF 权重塞进同一进程会直接顶破上限；
llama.cpp 官方发布预编译产物，作为数据文件进 PyInstaller onedir 最稳，不必为
Windows 编译 native 扩展；且 Provider 层本就是「HTTP 打本地服务」，llama-server
自带 OpenAI 兼容路由，形状完全吻合。

安全与健壮性要点（实现见各函数）：引擎只绑 ``127.0.0.1`` 并启用一次性 API Key
（见 :data:`_ENV_API_KEY`）；就绪轮询期间同步探子进程是否已退出（加载失败会秒退，
早失败优于等满超时）；子进程输出用于诊断但**必须脱敏**后才进错误文本（见
:func:`drain_output`）；被强杀（SIGKILL）会留下孤儿进程，故写 PID 文件供下次启动
清理，且只杀「可执行文件路径匹配本引擎」的进程（见 :func:`kill_orphan_from_pid_file`）。
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import socket
import subprocess  # noqa: S404 - 需要拉起随包分发的引擎子进程（见模块 docstring）
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING

import httpx

from app.core.logging import getLogger, redact

if TYPE_CHECKING:
    from collections.abc import Sequence

logger = getLogger("filemind.local_llm")

#: 引擎可执行文件路径覆盖（dev / CI / 自备引擎；与 FILEMIND_SIDECAR_BINARY 同一模式）
_ENV_BINARY = "FILEMIND_LLAMA_SERVER_BINARY"
#: 数据目录（与 app.main 同一约定，用于放 PID 文件）
_ENV_DATA_HOME = "FILEMIND_DATA_HOME"
#: 引擎 API Key 的环境变量名。
#:
#: **不用命令行传 key**：argv 在 ``ps`` / ``/proc/<pid>/cmdline`` 里对其他本地用户可见，
#: 而环境变量仅同用户与 root 可读——否则「给引擎加鉴权」只是把门钥匙挂在门口。
#: 引擎侧 ``LLAMA_API_KEY`` 等价于 ``--api-key``（实测：/health 公开、推理端点无 key 401）。
_ENV_API_KEY = "LLAMA_API_KEY"
#: 引擎可执行文件名（Windows 带 .exe）
_SERVER_NAME = "llama-server.exe" if sys.platform.startswith("win") else "llama-server"
#: 随包分发时的相对位置（PyInstaller ``datas`` 映射到 onedir 的 ``_internal`` 下）
_BUNDLED_REL = ("vendor", "llama")
#: PID 文件名（放在数据目录根，与数据库同级）
_PID_FILE = "local_llm.pid"

#: 引擎就绪等待上限（秒）：2GB GGUF 冷加载实测数秒，留足余量
READY_TIMEOUT = float(os.environ.get("FILEMIND_LLAMA_READY_TIMEOUT", "120"))
#: 就绪轮询间隔（秒）
READY_POLL_INTERVAL = 0.5
#: 上下文窗口（对齐 OLLAMA_NUM_CTX 的默认口径）
CONTEXT_SIZE = int(os.environ.get("FILEMIND_LLAMA_CTX", "8192") or "8192")
#: 推理线程数（0 = 交给 llama.cpp 自行按核数决定）
THREADS = int(os.environ.get("FILEMIND_LLAMA_THREADS", "0") or "0")
#: 健康探测超时（秒）：引擎启动期不响应属正常，故短超时 + 轮询
HEALTH_TIMEOUT = 2.0


class LocalLlmUnavailableError(Exception):
    """内置生成引擎不可用（后端未开启 / 二进制缺失 / 权重未下载 / 启动失败）。"""


def _env(name: str, default: str = "") -> str:
    """读取 env（去首尾空白）。"""
    return os.environ.get(name, "").strip() or default


def data_home() -> Path:
    """数据目录（与 ``app.main.DATA_HOME`` 同一约定）。"""
    return Path(_env(_ENV_DATA_HOME) or str(Path.home() / ".filemind"))


def pid_file_path() -> Path:
    """陈旧 PID 文件路径。"""
    return data_home() / _PID_FILE


def platform_key() -> str:
    """当前平台的目录键（与 ``scripts/fetch-llama-server.sh`` 的落盘目录一致）。"""
    machine = (
        os.uname().machine if hasattr(os, "uname") else os.environ.get("PROCESSOR_ARCHITECTURE", "")
    )
    arch = "arm64" if machine.lower() in {"arm64", "aarch64"} else "x64"
    os_key = "win" if sys.platform.startswith("win") else "macos"
    return f"{os_key}-{arch}"


def server_binary_path() -> Path | None:
    """解析引擎可执行文件路径：env 覆盖 → 随包产物 → dev 源码树。

    找不到时返回 ``None``（渲染成「引擎不可用」而不是抛异常，便于探测接口展示）。

    Returns:
        可执行文件路径；三处均未命中返回 ``None``。
    """
    override = _env(_ENV_BINARY)
    if override:
        return Path(override)
    # 打包态：PyInstaller onedir 的数据目录（_internal）下
    meipass = getattr(sys, "_MEIPASS", "")
    if meipass:
        bundled = Path(meipass).joinpath(*_BUNDLED_REL, _SERVER_NAME)
        if bundled.is_file():
            return bundled
    # dev：源码树内由 scripts/fetch-llama-server.sh 落盘
    dev = Path(__file__).resolve().parents[2] / "vendor" / "llama" / platform_key() / _SERVER_NAME
    return dev if dev.is_file() else None


def pick_free_port() -> int:
    """取一个本机空闲端口（绑定 0 让内核分配后立即释放）。

    极端并发下存在「取到端口到引擎真正监听」之间的竞争窗口，但启动由 service 层的
    生命周期锁串行化，且引擎启动失败会明确报错，风险可接受。
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def build_command(binary: Path, gguf: Path, port: int) -> list[str]:
    """构造 llama-server 启动参数（纯函数，便于单测）。

    只用长期稳定的长选项（``--model`` / ``--host`` / ``--port`` / ``--ctx-size``）；
    线程数为 0 时不传 ``--threads``（交给引擎按核数决定）。
    """
    command = [
        str(binary),
        "--model",
        str(gguf),
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
        "--ctx-size",
        str(CONTEXT_SIZE),
    ]
    if THREADS > 0:
        command += ["--threads", str(THREADS)]
    return command


def kill_orphan_from_pid_file() -> None:
    """清理上一次被强杀留下的孤儿引擎（只杀路径匹配的进程）。

    Sidecar 被 SIGKILL 时子进程会被 init 收养并继续占内存；写入 PID 文件是唯一能
    在下次启动时识别它们的线索。判定条件严格些：PID 存活 **且** 该进程的可执行
    文件路径等于我们当前的引擎路径，避免 PID 复用误杀无关程序。
    """
    path = pid_file_path()
    try:
        raw = path.read_text(encoding="utf-8").strip()
    except OSError:
        return
    if not raw.isdigit():
        _unlink_quietly(path)
        return
    pid = int(raw)
    if not _is_our_engine_alive(pid):
        _unlink_quietly(path)
        return
    logger.warning("local_llm.orphan_found", pid=pid)
    with contextlib.suppress(OSError, ProcessLookupError):
        os.kill(pid, 9 if not sys.platform.startswith("win") else 15)
    _unlink_quietly(path)


def _is_our_engine_alive(pid: int) -> bool:
    """PID 是否存活且可执行文件路径与本引擎一致（psutil 跨平台，取不到即保守放弃）。"""
    expected = server_binary_path()
    if pid <= 0 or expected is None:
        return False
    try:
        import psutil

        # exe() 对已退出进程抛 NoSuchProcess、对其他用户进程抛 AccessDenied，
        # 两种情况都会落到 except：无法确认是我们拉起的引擎就不动它
        return Path(psutil.Process(pid).exe()).resolve() == expected.resolve()
    except Exception:  # noqa: BLE001 - 平台差异大，取不到就保守放弃清理
        return False


def _unlink_quietly(path: Path) -> None:
    with contextlib.suppress(OSError):
        path.unlink(missing_ok=True)


def record_pid(pid: int) -> None:
    """记录引擎 PID 到数据目录（供下次启动清理孤儿进程；写失败不影响本次运行）。"""
    with contextlib.suppress(OSError):
        pid_file_path().write_text(str(pid), encoding="utf-8")


def spawn(command: Sequence[str], api_key: str) -> subprocess.Popen[bytes]:
    """拉起引擎子进程（stdout/stderr 合并落管道，供失败诊断）。

    API Key 经 env 传入（见 :data:`_ENV_API_KEY` 的说明：argv 对其他本地用户可见）。
    其余环境变量原样继承（NO_PROXY 等由 Rust 注入）。
    """
    env = {**os.environ, _ENV_API_KEY: api_key} if api_key else None
    return subprocess.Popen(  # noqa: S603 - 参数由本模块构造，非用户输入
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        env=env,
        # Windows：抑制控制台黑框（POSIX 忽略该参数）
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


async def probe_health(base_url: str, headers: dict[str, str]) -> bool:
    """探测引擎 ``/health`` 是否就绪（启动期不响应属正常）。

    实测该端点**不要求鉴权**（与 ``/v1/chat/completions`` 不同），但仍带上鉴权头：
    将来引擎收紧 /health 时不必回头改这里。
    """
    try:
        async with httpx.AsyncClient(timeout=HEALTH_TIMEOUT) as client:
            resp = await client.get(f"{base_url}/health", headers=headers)
            return resp.status_code == 200
    except httpx.HTTPError:
        return False


async def wait_ready(
    proc: subprocess.Popen[bytes], base_url: str, model: str, headers: dict[str, str]
) -> None:
    """轮询等待引擎就绪；子进程提前退出则带 stderr 尾部报错。

    ``headers`` 为健康探测要带的鉴权头（引擎将来收紧 ``/health`` 时无需回头改这里）。

    Raises:
        LocalLlmUnavailableError: 超时或子进程提前退出。
    """
    deadline = time.monotonic() + READY_TIMEOUT
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise LocalLlmUnavailableError(
                f"内置生成引擎启动即退出（exit={proc.returncode}）: {drain_output(proc)}"
            )
        if await probe_health(base_url, headers):
            return
        await asyncio.sleep(READY_POLL_INTERVAL)
    stop_process(proc)
    raise LocalLlmUnavailableError(f"内置生成引擎就绪超时（>{READY_TIMEOUT}s，model={model}）")


def drain_output(proc: subprocess.Popen[bytes]) -> str:
    """取子进程输出尾部（同步、非阻塞式读取；已退出的进程不会挂住）。

    **必须脱敏**：llama.cpp 启动日志里带完整路径（如
    ``load_model: loading model from /Users/<名>/.../xxx.gguf``），而这段文本会经
    ``LocalLlmUnavailableError`` → ``LLMUnavailableError`` → HTTP 错误响应直达设置页/
    聊天界面。日志出口（``app.core.logging``）本身有 redact，但响应体没有，故在此
    先用统一脱敏函数处理（与 Rust ``security/log_redact.rs`` 同一规则）。
    """
    stream = proc.stdout
    if stream is None:
        return ""
    chunks: list[bytes] = []
    with contextlib.suppress(Exception):
        while True:
            chunk = stream.read1(65536) if hasattr(stream, "read1") else stream.read(65536)
            if not chunk:
                break
            chunks.append(chunk)
    text = b"".join(chunks).decode("utf-8", errors="replace")
    return redact(text)[-800:]


def stop_process(proc: subprocess.Popen[bytes] | None) -> None:
    """尽力终止子进程（终止失败只记日志，不抛出）。"""
    if proc is None:
        return
    with contextlib.suppress(Exception):
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    _unlink_quietly(pid_file_path())
