"""T3 — 内置本地生成引擎（llama-server 子进程托管）单元测试。

分两层：`local_llm_engine`（产物定位与子进程原语，无状态）与 `local_llm_service`
（运行态与生命周期编排）。覆盖：启动参数构造、可执行文件解析、空闲卸载、孤儿清理的
安全边界（只杀路径匹配的进程）、以及「前置条件不满足时给出可操作错误」的四条路径
（后端未开启 / 二进制缺失 / 权重未下载 / 引擎启动即退出）。

真实子进程生命周期用**桩引擎**（临时目录里的 Python 脚本，实现 /health）验证，
仅在 POSIX 跑：Windows 上无法用 shebang 脚本当可执行文件（CI 的 Windows job 仍覆盖
上面的纯逻辑用例）。
"""

from __future__ import annotations

import os
import socket
import stat
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING

import httpx
import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import local_llm_engine as engine  # noqa: E402
from app.services import local_llm_service as svc  # noqa: E402
from app.services.model_specs import LLM_MODEL_NAME, resolve_spec  # noqa: E402

#: 桩引擎脚本：解析 --port 并实现 /health（启动即退出由环境变量 STUB_FAIL 触发）
_STUB_ENGINE = '''\
#!/usr/bin/env python3
"""最小桩引擎：只为验证进程托管逻辑，不做任何推理。"""
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

if os.environ.get("STUB_FAIL"):
    # 模拟 llama.cpp 真实启动日志：带完整权重路径（用于验证错误文本脱敏）
    if os.environ.get("STUB_PATH"):
        print(f"load_model: loading model from {os.environ['STUB_PATH']}", flush=True)
    print("stub engine: failed to load model", flush=True)
    sys.exit(3)

port = int(sys.argv[sys.argv.index("--port") + 1])
expected_key = os.environ.get("LLAMA_API_KEY", "")


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler 接口
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        elif self.path == "/v1/chat/completions":
            # 与真实引擎一致：推理端点要求 Bearer key
            if expected_key and self.headers.get("Authorization") != f"Bearer {expected_key}":
                self.send_error(401)
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"choices":[{"message":{"content":"ok"}}]}')
        else:
            self.send_error(404)

    def log_message(self, *args: object) -> None:
        return


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
'''


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """独立模型目录 / 数据目录，默认不暴露真实引擎，退出时确保子进程被停掉。

    默认把平台键改成不存在的值，使「源码树里没有引擎产物」成为确定前提——否则
    本机跑过 ``scripts/fetch-llama-server.sh`` 后，「引擎缺失」类用例会突然失效
    （曾经踩到：预期「二进制缺失」变成「权重未下载」）。需要引擎的用例用
    ``FILEMIND_LLAMA_SERVER_BINARY`` 显式覆盖（该分支优先于源码树探测）。
    """
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path / "models"))
    monkeypatch.setenv("FILEMIND_DATA_HOME", str(tmp_path / "home"))
    monkeypatch.delenv("FILEMIND_LLAMA_SERVER_BINARY", raising=False)
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "builtin")
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_MODEL", LLM_MODEL_NAME)
    monkeypatch.setattr(engine, "platform_key", lambda: "no-such-platform")
    svc.reset_state()
    yield
    svc.stop_server()
    svc.reset_state()


def _write_stub_engine(tmp_path: Path) -> Path:
    """写出可执行的桩引擎脚本，返回其路径。"""
    stub = tmp_path / "llama-server"
    stub.write_text(_STUB_ENGINE, encoding="utf-8")
    stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
    return stub


def _place_gguf(tmp_path: Path) -> Path:
    """在隔离模型目录里放一份（假）GGUF 权重，使「权重就绪」前置条件成立。"""
    gguf = resolve_spec(LLM_MODEL_NAME).root / resolve_spec(LLM_MODEL_NAME).weight
    gguf.parent.mkdir(parents=True, exist_ok=True)
    gguf.write_bytes(b"\x00" * 64)
    return gguf


# ------------------------------------------------------------------
# 纯逻辑：参数与路径解析
# ------------------------------------------------------------------


def test_build_command_uses_stable_long_options(tmp_path: Path) -> None:
    """启动参数只用长期稳定的长选项，且绑定 127.0.0.1（不对局域网暴露）。"""
    command = engine.build_command(tmp_path / "llama-server", tmp_path / "m.gguf", 51234)

    assert command[0].endswith("llama-server")
    assert command[command.index("--model") + 1].endswith("m.gguf")
    assert command[command.index("--host") + 1] == "127.0.0.1"
    assert command[command.index("--port") + 1] == "51234"
    assert "--ctx-size" in command
    # 线程数为 0（默认）时不传 --threads，交给引擎按核数决定
    assert "--threads" not in command


def test_build_command_includes_threads_when_configured(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """显式配置线程数时透传给引擎。"""
    monkeypatch.setattr(engine, "THREADS", 6)
    command = engine.build_command(tmp_path / "llama-server", tmp_path / "m.gguf", 1)
    assert command[command.index("--threads") + 1] == "6"


def test_binary_path_honours_env_override(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """env 覆盖优先（dev / CI / 自备引擎）。"""
    custom = tmp_path / "custom-llama-server"
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(custom))
    assert engine.server_binary_path() == custom


def test_binary_path_returns_none_when_absent(monkeypatch: pytest.MonkeyPatch) -> None:
    """既无 env 覆盖、随包产物与源码树也都没有 → None（渲染成「引擎不可用」）。"""
    monkeypatch.setattr(engine, "platform_key", lambda: "no-such-platform")
    assert engine.server_binary_path() is None


def test_pick_free_port_is_actually_free() -> None:
    """取到的端口可被立即绑定（避免与用户自己的 8080/11434 抢端口）。"""
    port = engine.pick_free_port()
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", port))


# ------------------------------------------------------------------
# 前置条件：不满足时给出可操作错误
# ------------------------------------------------------------------


async def test_ensure_server_requires_builtin_backend(monkeypatch: pytest.MonkeyPatch) -> None:
    """后端仍是 ollama → 明确拒绝并提示切换，不静默去起引擎。"""
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "ollama")
    with pytest.raises(svc.LocalLlmUnavailableError, match="未开启"):
        await svc.ensure_server()


async def test_ensure_server_rejects_missing_binary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎二进制缺失 → 提示未随包分发 / 未执行 fetch 脚本。"""
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(tmp_path / "absent"))
    with pytest.raises(svc.LocalLlmUnavailableError, match="可执行文件缺失"):
        await svc.ensure_server()


async def test_ensure_server_rejects_missing_weights(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """二进制在但 GGUF 未下载 → 指向设置页的下载入口。"""
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(_write_stub_engine(tmp_path)))
    with pytest.raises(svc.LocalLlmUnavailableError, match="权重未下载"):
        await svc.ensure_server()


async def test_ensure_server_reports_early_exit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎启动即退出（如权重损坏）→ 报错带上退出码与 stderr 尾部。"""
    if sys.platform.startswith("win"):
        pytest.skip("桩引擎依赖 shebang，Windows 不适用")
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    monkeypatch.setenv("STUB_FAIL", "1")

    with pytest.raises(svc.LocalLlmUnavailableError) as excinfo:
        await svc.ensure_server()

    message = str(excinfo.value)
    assert "启动即退出" in message
    assert "failed to load model" in message  # stderr 尾部带进异常，便于定位


async def test_engine_error_message_is_redacted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎 stderr 里的真实路径必须脱敏：该文本会经错误响应直达界面。

    llama.cpp 启动日志形如 ``load_model: loading model from /Users/<名>/.../x.gguf``，
    若原样透出，等于把用户目录结构暴露在界面与错误链里（日志出口有 redact，响应体没有）。
    """
    if sys.platform.startswith("win"):
        pytest.skip("桩引擎依赖 shebang，Windows 不适用")
    secret_dir = tmp_path / "private" / "user-docs"
    secret_dir.mkdir(parents=True)
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    monkeypatch.setenv("STUB_FAIL", "1")
    monkeypatch.setenv("STUB_PATH", str(secret_dir / "secret.gguf"))

    with pytest.raises(svc.LocalLlmUnavailableError) as excinfo:
        await svc.ensure_server()

    message = str(excinfo.value)
    assert str(tmp_path) not in message, "错误文本不应包含真实路径"
    assert "secret.gguf" in message, "保留末级文件名便于定位"


def test_engine_prerequisites_ignores_backend_config(monkeypatch: pytest.MonkeyPatch) -> None:
    """前置条件检查与后端配置解耦：配置为 ollama 时同样如实报告。

    回归点：原实现按配置短路返回「后端为 ollama」，而本函数的唯一调用方是探测的回落
    判定——于是「配置 ollama + Ollama 不可用」这条路上回落永不发生，未装 Ollama 的
    机器上默认配置问答不了（与 T3 目标相悖）。
    """
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "ollama")
    assert svc.engine_prerequisites() == (False, "引擎可执行文件缺失")

    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "builtin")
    assert svc.engine_prerequisites() == (False, "引擎可执行文件缺失")


def test_engine_prerequisites_reports_missing_weights(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎在但 GGUF 未下载 → 报权重缺失（前置的第二道）。"""
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(_write_stub_engine(tmp_path)))
    assert svc.engine_prerequisites() == (False, f"权重未下载（{LLM_MODEL_NAME}）")


def test_engine_prerequisites_ok_when_engine_and_weights_present(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎产物 + GGUF 齐备 → 前置满足（这时才允许回落 builtin）。"""
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(_write_stub_engine(tmp_path)))
    _place_gguf(tmp_path)
    assert svc.engine_prerequisites() == (True, "")


# ------------------------------------------------------------------
# 子进程生命周期（POSIX：用桩引擎）
# ------------------------------------------------------------------

_POSIX_ONLY = pytest.mark.skipif(
    sys.platform.startswith("win"), reason="桩引擎依赖 shebang，Windows 不适用"
)


@_POSIX_ONLY
async def test_ensure_server_allows_fallback_when_prerequisites_ready(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """配置仍是 ollama 但前置齐备（探测已回落到内置）→ 允许拉起引擎。

    回归：原先按配置硬拒（「内置生成后端未开启（当前 ollama）」），把探测回落路径整个
    挡死——E2E-003 内置引擎路径实测就是卡在这里，机器上没装 Ollama 就永远问答不了。
    """
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "ollama")

    base_url = await svc.ensure_server()

    assert base_url.startswith("http://127.0.0.1:")
    assert svc.status().running


@_POSIX_ONLY
async def test_ensure_server_starts_and_is_idempotent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """拉起引擎 → /health 可用 → 二次调用复用同一进程（不重复占内存）。"""
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))

    base_url = await svc.ensure_server()
    first = svc.status()

    assert base_url.startswith("http://127.0.0.1:")
    assert first.running
    assert first.model == LLM_MODEL_NAME
    assert first.pid is not None
    async with httpx.AsyncClient(timeout=3) as client:
        resp = await client.get(f"{base_url}/health")
    assert resp.status_code == 200

    assert await svc.ensure_server() == base_url
    assert svc.status().pid == first.pid, "同一模型不应重启引擎"


@_POSIX_ONLY
async def test_idle_unload_stops_engine(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """空闲超阈值 → 停掉子进程释放内存（下次取用时惰性触发）。"""
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    await svc.ensure_server()
    assert svc.status().running

    monkeypatch.setattr(svc, "IDLE_UNLOAD", 0)
    time.sleep(0.01)  # 让 last_used 早于阈值判定
    svc._unload_if_idle()

    assert not svc.status().running
    assert svc.status().pid is None


@_POSIX_ONLY
async def test_stop_server_is_idempotent(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """重复停止不抛错（shutdown 与 atexit 都会调用）。"""
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    await svc.ensure_server()

    svc.stop_server()
    svc.stop_server()

    assert svc.status() == svc.LocalLlmStatus(
        running=False, base_url="", model="", pid=None, last_error=""
    )


@_POSIX_ONLY
async def test_start_failure_does_not_leave_process(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """启动失败必须清干净状态：否则后续请求会命中一个已死的 base_url。"""
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    monkeypatch.setenv("STUB_FAIL", "1")

    with pytest.raises(svc.LocalLlmUnavailableError):
        await svc.ensure_server()

    stopped = svc.status()
    assert not stopped.running
    assert stopped.pid is None
    # 失败原因留在状态里，供设置页与日志展示
    assert "启动即退出" in stopped.last_error


# ------------------------------------------------------------------
# 孤儿清理：安全边界
# ------------------------------------------------------------------


@_POSIX_ONLY
async def test_engine_requires_our_api_key(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """引擎推理端点按一次性 key 鉴权：不带 key 401，带我们的 key 才 200。

    引擎监听本地端口，不鉴权则本机任何进程都能拿它做推理（白占 CPU/约 2GB 内存）。
    桩引擎按真实引擎的行为实现鉴权，本用例同时验证「key 经 env 正确送达引擎」
    与「Provider 侧带的头与 key 一致」。
    """
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    base_url = await svc.ensure_server()

    async with httpx.AsyncClient(timeout=3) as client:
        bare = await client.get(f"{base_url}/v1/chat/completions")
        with_key = await client.get(f"{base_url}/v1/chat/completions", headers=svc.auth_headers())

    assert bare.status_code == 401, "不带 key 必须被拒（否则本机任意进程可用）"
    assert with_key.status_code == 200


async def test_api_key_not_exposed_via_argv(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """key 不得出现在命令行：argv 在 ps / /proc 里对其他本地用户可见。"""
    command = engine.build_command(tmp_path / "llama-server", tmp_path / "m.gguf", 12345)

    assert "--api-key" not in command
    assert not any("Bearer" in arg for arg in command)


async def test_auth_headers_empty_when_not_running(monkeypatch: pytest.MonkeyPatch) -> None:
    """引擎未运行时不给鉴权头（无 key 可给，也不该误报）。"""
    assert svc.auth_headers() == {}


@_POSIX_ONLY
async def test_api_key_rotates_per_start(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """每次启动重新生成 key：旧 key 在重启后失效。"""
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))

    await svc.ensure_server()
    first = svc.auth_headers()
    svc.stop_server()
    await svc.ensure_server()

    assert first != svc.auth_headers()
    assert svc.auth_headers().get("Authorization", "").startswith("Bearer ")


def test_orphan_cleanup_ignores_garbage_pid_file(monkeypatch: pytest.MonkeyPatch) -> None:
    """PID 文件内容非法 → 只删文件，不做任何杀进程动作。"""
    path = engine.pid_file_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("not-a-pid", encoding="utf-8")

    engine.kill_orphan_from_pid_file()

    assert not path.exists()


def test_orphan_cleanup_never_kills_unrelated_process(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """PID 存活但可执行文件不是本引擎（PID 复用）→ 不杀，仅清掉陈旧文件。

    这是本模块最危险的边界：清理逻辑若只按 PID 存活就开杀，会误杀用户进程。
    """
    stub = _write_stub_engine(tmp_path)
    _place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    path = engine.pid_file_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(str(os.getpid()), encoding="utf-8")  # 本测试进程：路径不匹配

    engine.kill_orphan_from_pid_file()

    assert not path.exists()
    # 进程仍活着（能继续执行就是最好的证据）
    assert os.getpid() > 0


def test_orphan_cleanup_handles_missing_pid_file(monkeypatch: pytest.MonkeyPatch) -> None:
    """没有 PID 文件时是 no-op（首次启动的常态）。"""
    engine.kill_orphan_from_pid_file()
    assert not engine.pid_file_path().exists()
