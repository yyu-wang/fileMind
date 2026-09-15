"""T2 — services.model_download_service 单元测试。

覆盖：待下载文件清单、就绪判定、初始/就绪状态、成功下载并落盘、
镜像切换重试、3 次用尽置 failed（含 .part 清理）、下载中幂等、
进度字节与总量上报、HEAD 无 Content-Length 时的不确定进度、
失败后手动重试。

全部用例用假 httpx 客户端（按 URL 前缀模拟各镜像的行为），不发起真实网络请求。
"""

from __future__ import annotations

import asyncio
import ssl
import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest import mock

import httpx
import pytest

if TYPE_CHECKING:
    from collections.abc import AsyncIterator, Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import model_download_service as svc  # noqa: E402

MODEL = "bge-large-zh-v1.5"
CONTENT = b"x" * 512


class _FakeResponse:
    """假响应：流式吐 chunk，HEAD 只给 headers。"""

    def __init__(self, chunks: list[bytes], headers: dict[str, str], status: int = 200) -> None:
        self._chunks = chunks
        self.headers = headers
        self.status_code = status

    def raise_for_status(self) -> None:
        """状态码 >= 400 时抛 HTTPStatusError（对齐 httpx 语义）。"""
        if self.status_code >= 400:
            raise httpx.HTTPStatusError(
                "boom",
                request=httpx.Request("GET", "http://fake"),
                response=None,  # type: ignore[arg-type]
            )

    async def aiter_bytes(self, chunk_size: int = 0) -> AsyncIterator[bytes]:
        """按给定的 chunk 依次产出。"""
        for chunk in self._chunks:
            yield chunk


class _FakeStream:
    """``client.stream(...)`` 返回的异步上下文管理器。"""

    def __init__(self, response: _FakeResponse) -> None:
        self._response = response

    async def __aenter__(self) -> _FakeResponse:
        return self._response

    async def __aexit__(self, *exc: object) -> bool:
        return False


class _FakeClient:
    """按镜像前缀分派的假 httpx 客户端。"""

    def __init__(
        self,
        *,
        fail_mirrors: frozenset[str] = frozenset(),
        head_without_length: bool = False,
    ) -> None:
        self._fail_mirrors = fail_mirrors
        self._head_without_length = head_without_length
        self.head_calls: list[str] = []
        self.range_calls: list[str] = []
        self.stream_calls: list[str] = []

    async def __aenter__(self) -> _FakeClient:
        return self

    async def __aexit__(self, *exc: object) -> bool:
        return False

    def _fails(self, url: str) -> bool:
        return any(url.startswith(mirror) for mirror in self._fail_mirrors)

    async def head(self, url: str) -> _FakeResponse:
        """HEAD：失败镜像抛连接错误；否则返回（可选缺失的）Content-Length。"""
        self.head_calls.append(url)
        if self._fails(url):
            raise httpx.ConnectError("mirror down")
        headers = {} if self._head_without_length else {"content-length": str(len(CONTENT))}
        return _FakeResponse([], headers)

    async def get(self, url: str, headers: dict[str, str] | None = None) -> _FakeResponse:
        """Range 探测：HEAD 缺 Content-Length 时的兜底路径。"""
        self.range_calls.append(url)
        if self._fails(url):
            raise httpx.ConnectError("mirror down")
        return _FakeResponse([], {"content-range": f"bytes 0-0/{len(CONTENT)}"})

    def stream(self, method: str, url: str) -> _FakeStream:
        """GET 流：失败镜像返回 503（raise_for_status 抛错）。"""
        self.stream_calls.append(url)
        if self._fails(url):
            return _FakeStream(_FakeResponse([], {}, status=503))
        return _FakeStream(_FakeResponse([CONTENT], {}))


def _patch_client(monkeypatch: pytest.MonkeyPatch, client: _FakeClient) -> None:
    """把服务模块内的 httpx 换成只含 AsyncClient 的替身（保留异常类型）。"""
    monkeypatch.setattr(
        svc,
        "httpx",
        SimpleNamespace(
            HTTPError=httpx.HTTPError,
            AsyncClient=lambda **kwargs: client,
        ),
    )


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """每个用例独立模型目录 + 干净状态。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    monkeypatch.setattr(svc, "MAX_ATTEMPTS", 3)
    svc.reset_state()
    yield
    svc.reset_state()


async def _run_download(model: str = MODEL) -> svc.ModelDownloadStatusResponse:
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
    client = _FakeClient()
    _patch_client(monkeypatch, client)
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
    client = _FakeClient(fail_mirrors=frozenset({svc.MIRRORS[0]}))
    _patch_client(monkeypatch, client)
    status = await _run_download()

    assert status.status == "ready"
    assert status.attempt == 2
    assert status.mirror == svc.MIRRORS[1]
    assert any(url.startswith(svc.MIRRORS[1]) for url in client.stream_calls)


async def test_download_fails_after_max_attempts_and_cleans_partials(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """全部镜像均失败 → attempt 用尽后 failed，错误说明重试次数，无 .part 残留。"""
    client = _FakeClient(fail_mirrors=frozenset(svc.MIRRORS))
    _patch_client(monkeypatch, client)
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
    failing = _FakeClient(fail_mirrors=frozenset(svc.MIRRORS))
    _patch_client(monkeypatch, failing)
    assert (await _run_download()).status == "failed"

    _patch_client(monkeypatch, _FakeClient())
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
    client = _FakeClient(head_without_length=True)
    _patch_client(monkeypatch, client)
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
    client = _FakeClient(head_without_length=True, fail_mirrors=frozenset({"https://"}))
    _patch_client(monkeypatch, client)
    size = await svc._probe_size(client, "https://hf-mirror.com/x/y")  # noqa: SLF001
    assert size is None


@pytest.mark.parametrize(
    ("header", "expected"),
    [
        ("bytes 0-0/12345", 12345),
        ("bytes 0-0/*", None),
        ("bytes 0-0/", None),
        ("garbage", None),
        (None, None),
    ],
)
def test_parse_content_range_total(header: str | None, expected: int | None) -> None:
    """Content-Range 解析：正常、未知总量（*）、残缺、垃圾值、缺失头部。"""
    assert svc._parse_content_range_total(header) == expected  # noqa: SLF001


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
    client = _FakeClient()
    _patch_client(monkeypatch, client)

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


def test_cleanup_partials_removes_leftovers(tmp_path: Path) -> None:
    """失败重试前的清理：删除 .part 残留，不影响已就绪文件。"""
    root = tmp_path / MODEL
    root.mkdir(parents=True)
    (root / "tokenizer.json").write_bytes(b"ok")
    (root / "tokenizer.json.part").write_bytes(b"half")
    svc._cleanup_partials(root, ("tokenizer.json",))  # noqa: SLF001
    assert not (root / "tokenizer.json.part").exists()
    assert (root / "tokenizer.json").exists()


def test_ssl_context_caps_tls12_by_default(monkeypatch: pytest.MonkeyPatch) -> None:
    """默认把 TLS 上限压到 1.2（TLS 1.3 访问 hf-mirror 必现 BAD_RECORD_MAC）。"""
    monkeypatch.delenv("FILEMIND_MODEL_TLS_MAX", raising=False)
    assert svc._ssl_context().maximum_version == ssl.TLSVersion.TLSv1_2  # noqa: SLF001


def test_ssl_context_allows_tls13_when_requested(monkeypatch: pytest.MonkeyPatch) -> None:
    """``FILEMIND_MODEL_TLS_MAX=1.3`` 时恢复默认协商（不设上限）。"""
    monkeypatch.setenv("FILEMIND_MODEL_TLS_MAX", "1.3")
    assert svc._ssl_context().maximum_version == ssl.TLSVersion.MAXIMUM_SUPPORTED  # noqa: SLF001


def test_download_one_writes_whole_file_atomically(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """单文件下载：整文件写入后原子改名，结束后无 .part 残留。"""
    client = _FakeClient()
    state = svc._state_for(MODEL)  # noqa: SLF001
    dest = tmp_path / "one.bin"

    async def _go() -> None:
        await svc._download_one(  # noqa: SLF001
            client,
            svc.MIRRORS[0],
            MODEL,
            "x/one.bin",
            dest,
            state,
            count_progress=True,
        )

    with mock.patch.object(svc.os, "replace", wraps=svc.os.replace) as replace:
        asyncio.run(_go())

    assert dest.read_bytes() == CONTENT
    assert replace.call_count == 1
    assert state.downloaded_bytes == len(CONTENT)
    assert not (tmp_path / "one.bin.part").exists()


def test_download_one_skips_progress_for_aux_files(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """count_progress=False 时不计入进度（辅助文件不参与进度口径）。"""
    client = _FakeClient()
    state = svc._state_for(MODEL)  # noqa: SLF001
    dest = tmp_path / "aux.bin"

    async def _go() -> None:
        await svc._download_one(  # noqa: SLF001
            client,
            svc.MIRRORS[0],
            MODEL,
            "tokenizer.json",
            dest,
            state,
            count_progress=False,
        )

    asyncio.run(_go())
    assert dest.read_bytes() == CONTENT
    assert state.downloaded_bytes == 0
