"""T3 — 内置本地生成引擎运行态测试：子进程生命周期 / 鉴权 / 孤儿清理。

从 `test_local_llm_service.py`（469 行）拆出：该文件覆盖「纯逻辑 + 前置条件 + 运行态」
三块，运行态部分需真实子进程（桩引擎），与纯逻辑用例的失败语义完全不同，故分文件。
纯逻辑与前置条件用例仍在 `test_local_llm_service.py`；共享工具见 `local_llm_support.py`。

真实子进程生命周期用**桩引擎**（临时目录里的 Python 脚本，实现 /health）验证，
仅在 POSIX 跑：Windows 上无法用 shebang 脚本当可执行文件（CI 的 Windows job 仍覆盖
纯逻辑用例）。
"""

from __future__ import annotations

import os
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
from app.services.model_specs import LLM_MODEL_NAME  # noqa: E402
from tests.local_llm_support import (  # noqa: E402
    POSIX_ONLY,
    place_gguf,
    write_stub_engine,
)


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """独立模型目录 / 数据目录，默认不暴露真实引擎，退出时确保子进程被停掉。

    与 test_local_llm_service.py 的同名夹具同源（本仓约定 fixtures 由各测试文件自持，
    不跨文件导入）。
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
# 子进程生命周期（POSIX：用桩引擎）
# ------------------------------------------------------------------

_POSIX_ONLY = pytest.mark.skipif(
    sys.platform.startswith("win"), reason="桩引擎依赖 shebang，Windows 不适用"
)


@POSIX_ONLY
async def test_ensure_server_allows_fallback_when_prerequisites_ready(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """配置仍是 ollama 但前置齐备（探测已回落到内置）→ 允许拉起引擎。

    回归：原先按配置硬拒（「内置生成后端未开启（当前 ollama）」），把探测回落路径整个
    挡死——E2E-003 内置引擎路径实测就是卡在这里，机器上没装 Ollama 就永远问答不了。
    """
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    monkeypatch.setenv("FILEMIND_LOCAL_LLM_BACKEND", "ollama")

    base_url = await svc.ensure_server()

    assert base_url.startswith("http://127.0.0.1:")
    assert svc.status().running


@POSIX_ONLY
async def test_ensure_server_starts_and_is_idempotent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """拉起引擎 → /health 可用 → 二次调用复用同一进程（不重复占内存）。"""
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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


@POSIX_ONLY
async def test_idle_unload_stops_engine(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """空闲超阈值 → 停掉子进程释放内存（下次取用时惰性触发）。"""
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    await svc.ensure_server()
    assert svc.status().running

    monkeypatch.setattr(svc, "IDLE_UNLOAD", 0)
    time.sleep(0.01)  # 让 last_used 早于阈值判定
    svc._unload_if_idle()

    assert not svc.status().running
    assert svc.status().pid is None


@POSIX_ONLY
async def test_stop_server_is_idempotent(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """重复停止不抛错（shutdown 与 atexit 都会调用）。"""
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
    monkeypatch.setenv("FILEMIND_LLAMA_SERVER_BINARY", str(stub))
    await svc.ensure_server()

    svc.stop_server()
    svc.stop_server()

    assert svc.status() == svc.LocalLlmStatus(
        running=False, base_url="", model="", pid=None, last_error=""
    )


@POSIX_ONLY
async def test_start_failure_does_not_leave_process(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """启动失败必须清干净状态：否则后续请求会命中一个已死的 base_url。"""
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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


@POSIX_ONLY
async def test_engine_requires_our_api_key(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """引擎推理端点按一次性 key 鉴权：不带 key 401，带我们的 key 才 200。

    引擎监听本地端口，不鉴权则本机任何进程都能拿它做推理（白占 CPU/约 2GB 内存）。
    桩引擎按真实引擎的行为实现鉴权，本用例同时验证「key 经 env 正确送达引擎」
    与「Provider 侧带的头与 key 一致」。
    """
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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


@POSIX_ONLY
async def test_api_key_rotates_per_start(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """每次启动重新生成 key：旧 key 在重启后失效。"""
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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
    stub = write_stub_engine(tmp_path)
    place_gguf(tmp_path)
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
