"""T3 — 内置本地生成引擎（llama-server 子进程托管）单元测试：纯逻辑与前置条件。

覆盖：启动参数构造、可执行文件解析，以及「前置条件不满足时给出可操作错误」的四条
路径（后端未开启 / 二进制缺失 / 权重未下载 / 引擎启动即退出）。

运行态用例（真实子进程生命周期 / 鉴权 / 孤儿清理）拆到
`test_local_llm_service_runtime.py`——那部分需桩引擎且仅 POSIX 可跑；共享工具见
`local_llm_support.py`。
"""

from __future__ import annotations

import socket
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import local_llm_engine as engine  # noqa: E402
from app.services import local_llm_service as svc  # noqa: E402
from app.services.model_specs import LLM_MODEL_NAME  # noqa: E402
from tests.local_llm_support import place_gguf, write_stub_engine  # noqa: E402


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
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(write_stub_engine(tmp_path)))
    with pytest.raises(svc.LocalLlmUnavailableError, match="权重未下载"):
        await svc.ensure_server()


async def test_ensure_server_reports_early_exit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎启动即退出（如权重损坏）→ 报错带上退出码与 stderr 尾部。"""
    if sys.platform.startswith("win"):
        pytest.skip("桩引擎依赖 shebang，Windows 不适用")
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(write_stub_engine(tmp_path)))
    assert svc.engine_prerequisites() == (False, f"权重未下载（{LLM_MODEL_NAME}）")


def test_engine_prerequisites_ok_when_engine_and_weights_present(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """引擎产物 + GGUF 齐备 → 前置满足（这时才允许回落 builtin）。"""
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(write_stub_engine(tmp_path)))
    place_gguf(tmp_path)
    assert svc.engine_prerequisites() == (True, "")
