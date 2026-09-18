"""T2 — services.model_download_service 单元测试。

覆盖：待下载文件清单、就绪判定、初始/就绪状态、成功下载并落盘、
镜像切换重试、3 次用尽置 failed（含 .part 清理）、下载中幂等、
进度字节与总量上报、HEAD 无 Content-Length 时的不确定进度、
失败后手动重试，以及 ensure_downloaded 的短路与 reset_state。

传输层内部用例（Content-Range 解析 / TLS 上限 / 单文件原子写）拆到
`test_model_download_transfer.py`；共享假件见 `model_download_fakes.py`。

全部用例用假 httpx 客户端（按 URL 前缀模拟各镜像的行为），不发起真实网络请求。
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

    from app.models import ModelDownloadStatusResponse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import model_download_service as svc  # noqa: E402
from app.services import model_download_transfer as transfer  # noqa: E402
from tests.model_download_fakes import (  # noqa: E402
    CONTENT,
    MODEL,
    FakeClient,
    patch_client,
)


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """每个用例独立模型目录 + 干净状态。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    monkeypatch.setattr(svc, "MAX_ATTEMPTS", 3)
    svc.reset_state()
    yield
    svc.reset_state()


async def _run_download(model: str = MODEL) -> ModelDownloadStatusResponse:
    """启动下载并等待后台任务结束，返回最终状态。"""
    await svc.ensure_downloaded(model)
    await svc._tasks[model]  # noqa: SLF001  测试内需等待后台任务收敛
    return svc.get_status(model)


# ------------------------------------------------------------------
# 清单与就绪判定
# ------------------------------------------------------------------


def test_files_for_includes_weight_and_tokenizer() -> None:
    """清单含注册表声明的 ONNX 权重与 tokenizer/config 辅助文件。"""
    files = svc.files_for(MODEL)
    assert files[0] == "onnx/model_quantized.onnx"
    assert "tokenizer.json" in files
    assert "config.json" in files


def test_files_for_unknown_model_raises() -> None:
    """未注册模型 → ValueError（路由层转 400）。"""
    with pytest.raises(ValueError, match="未知 Embedding 模型"):
        svc.files_for("not-a-model")


def test_model_ready_false_when_missing(tmp_path: Path) -> None:
    """文件缺失 → 未就绪。"""
    assert svc.model_ready(MODEL) is False


def test_model_ready_false_when_empty_file(tmp_path: Path) -> None:
    """存在但为空文件 → 未就绪（避免半截/空文件被当作就绪）。"""
    root = tmp_path / MODEL
    for name in svc.files_for(MODEL):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"" if name == "tokenizer.json" else b"data")
    assert svc.model_ready(MODEL) is False


def test_model_ready_true_when_all_present(tmp_path: Path) -> None:
    """全部文件存在且非空 → 就绪。"""
    root = tmp_path / MODEL
    for name in svc.files_for(MODEL):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"data")
    assert svc.model_ready(MODEL) is True


def test_model_ready_unknown_model_is_false() -> None:
    """未知模型 → False（不抛错，供状态查询安全调用）。"""
    assert svc.model_ready("nope") is False


# ------------------------------------------------------------------
# 状态查询
# ------------------------------------------------------------------


def test_get_status_idle_before_download() -> None:
    """未开始且本地无文件 → idle。"""
    status = svc.get_status(MODEL)
    assert status.status == "idle"
    assert status.mirror is None
    assert status.attempt == 0


def test_get_status_ready_when_files_exist(tmp_path: Path) -> None:
    """本地文件已齐备 → 直接 ready（不触发下载）。"""
    root = tmp_path / MODEL
    for name in svc.files_for(MODEL):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"data")
    assert svc.get_status(MODEL).status == "ready"


def test_get_status_unknown_model_raises() -> None:
    """未知模型 → ValueError。"""
    with pytest.raises(ValueError):
        svc.get_status("nope")


# ------------------------------------------------------------------
# 下载：成功 / 镜像切换 / 用尽
# ------------------------------------------------------------------


async def test_download_success_writes_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """下载成功：全部文件落盘、状态 ready、进度口径为权重文件。"""
    client = FakeClient()
    patch_client(monkeypatch, client)
    status = await _run_download()

    assert status.status == "ready"
    assert status.error is None
    assert status.attempt == 1
    assert status.mirror == svc.MIRRORS[0]

    # 进度只统计权重文件（总大小 = 单个文件大小）
    assert status.total_bytes == len(CONTENT)
    assert status.downloaded_bytes == len(CONTENT)
    assert svc.model_ready(MODEL) is True
    # 每个文件都从首个镜像拉取
    assert len(client.stream_calls) == len(svc.files_for(MODEL))
    assert all(url.startswith(svc.MIRRORS[0]) for url in client.stream_calls)


async def test_download_falls_back_to_second_mirror(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """首个镜像失败 → 自动切换下一个镜像并成功（attempt=2）。"""
    client = FakeClient(fail_mirrors=frozenset({svc.MIRRORS[0]}))
    patch_client(monkeypatch, client)
    status = await _run_download()

    assert status.status == "ready"
    assert status.attempt == 2
    assert status.mirror == svc.MIRRORS[1]
    assert any(url.startswith(svc.MIRRORS[1]) for url in client.stream_calls)


async def test_download_fails_after_max_attempts_and_cleans_partials(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """全部镜像均失败 → attempt 用尽后 failed，错误说明重试次数，无 .part 残留。"""
    client = FakeClient(fail_mirrors=frozenset(svc.MIRRORS))
    patch_client(monkeypatch, client)
    status = await _run_download()

    assert status.status == "failed"
    assert status.attempt == svc.MAX_ATTEMPTS == 3
    assert status.error is not None
    assert "3 次" in status.error
    assert not list((tmp_path / MODEL).rglob("*.part"))


async def test_manual_retry_after_failure_can_succeed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """failed 后用户再次点击 → 重新计数并成功。"""
    failing = FakeClient(fail_mirrors=frozenset(svc.MIRRORS))
    patch_client(monkeypatch, failing)
    assert (await _run_download()).status == "failed"

    patch_client(monkeypatch, FakeClient())
    status = await _run_download()
    assert status.status == "ready"
    assert status.attempt == 1


async def test_download_idempotent_while_running(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """下载中重复调用不重复启动任务（避免并发重复下载）。"""
    started: list[str] = []

    async def _slow(model: str) -> None:
        started.append(model)
        await asyncio.sleep(0.05)

    monkeypatch.setattr(svc, "_download_all", _slow)
    await svc.ensure_downloaded(MODEL)
    # 让后台任务真正进入运行态，再验证第二次调用不重复启动
    await asyncio.sleep(0.01)
    second = await svc.ensure_downloaded(MODEL)
    assert started == [MODEL]
    assert second.status == "downloading"
    await svc._tasks[MODEL]  # noqa: SLF001


async def test_total_falls_back_to_content_range_when_head_lacks_length(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """HEAD 缺 Content-Length → 用 Range 的 Content-Range 拿到总量（进度可百分比）。"""
    client = FakeClient(head_without_length=True)
    patch_client(monkeypatch, client)
    status = await _run_download()

    assert status.status == "ready"
    assert status.total_bytes == len(CONTENT)
    assert status.downloaded_bytes == status.total_bytes
    # 只有权重文件需要探测大小（辅助文件不参与进度口径）
    assert len(client.range_calls) == 1


async def test_probe_size_none_when_size_unavailable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """HEAD 与 Range 都拿不到大小 → None（前端显示不确定进度）。"""
    client = FakeClient(head_without_length=True, fail_mirrors=frozenset({"https://"}))
    patch_client(monkeypatch, client)
    size = await transfer._probe_size(client, "https://hf-mirror.com/x/y")  # noqa: SLF001
    assert size is None


async def test_ensure_downloaded_unknown_model_raises() -> None:
    """未知模型 → ValueError。"""
    with pytest.raises(ValueError):
        await svc.ensure_downloaded("nope")


async def test_ensure_downloaded_short_circuits_when_ready(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """已就绪时直接返回 ready，不发起任何网络请求。"""
    root = tmp_path / MODEL
    for name in svc.files_for(MODEL):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"data")
    client = FakeClient()
    patch_client(monkeypatch, client)

    status = await svc.ensure_downloaded(MODEL)
    assert status.status == "ready"
    assert client.stream_calls == []


def test_reset_state_clears_status() -> None:
    """reset_state 清空记录（测试隔离用）。"""
    svc.get_status(MODEL)
    assert svc._states  # noqa: SLF001
    svc.reset_state()
    assert not svc._states  # noqa: SLF001
    assert not svc._tasks  # noqa: SLF001
