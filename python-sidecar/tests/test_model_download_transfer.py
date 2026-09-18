"""T2 — services.model_download_transfer 单元测试：Content-Range 解析 / TLS 上限 / 单文件原子写。

从 `test_model_download_service.py`（462 行）拆出：这些用例验证传输层内部实现
（`transfer._parse_content_range_total` / `_ssl_context` / `_download_one` /
`_cleanup_partials`），与「清单 / 状态 / 下载编排」的用例失败语义不同，故分文件。
服务级用例仍在 `test_model_download_service.py`；共享假件见 `model_download_fakes.py`。
"""

from __future__ import annotations

import asyncio
import ssl
import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import model_download_service as svc  # noqa: E402
from app.services import model_download_transfer as transfer  # noqa: E402
from tests.model_download_fakes import CONTENT, MODEL, FakeClient  # noqa: E402


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """每个用例独立模型目录 + 干净状态（与 test_model_download_service.py 同名夹具同源）。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    monkeypatch.setattr(svc, "MAX_ATTEMPTS", 3)
    svc.reset_state()
    yield
    svc.reset_state()


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
    assert transfer._parse_content_range_total(header) == expected  # noqa: SLF001


def test_cleanup_partials_removes_leftovers(tmp_path: Path) -> None:
    """失败重试前的清理：删除 .part 残留，不影响已就绪文件。"""
    root = tmp_path / MODEL
    root.mkdir(parents=True)
    (root / "tokenizer.json").write_bytes(b"ok")
    (root / "tokenizer.json.part").write_bytes(b"half")
    transfer._cleanup_partials(root, ("tokenizer.json",))  # noqa: SLF001
    assert not (root / "tokenizer.json.part").exists()
    assert (root / "tokenizer.json").exists()


def test_ssl_context_caps_tls12_by_default(monkeypatch: pytest.MonkeyPatch) -> None:
    """默认把 TLS 上限压到 1.2（TLS 1.3 访问 hf-mirror 必现 BAD_RECORD_MAC）。"""
    monkeypatch.delenv("FILEMIND_MODEL_TLS_MAX", raising=False)
    assert transfer._ssl_context().maximum_version == ssl.TLSVersion.TLSv1_2  # noqa: SLF001


def test_ssl_context_allows_tls13_when_requested(monkeypatch: pytest.MonkeyPatch) -> None:
    """``FILEMIND_MODEL_TLS_MAX=1.3`` 时恢复默认协商（不设上限）。"""
    monkeypatch.setenv("FILEMIND_MODEL_TLS_MAX", "1.3")
    assert transfer._ssl_context().maximum_version == ssl.TLSVersion.MAXIMUM_SUPPORTED  # noqa: SLF001


def test_download_one_writes_whole_file_atomically(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """单文件下载：整文件写入后原子改名，结束后无 .part 残留。"""
    client = FakeClient()
    state = svc._state_for(MODEL)  # noqa: SLF001
    dest = tmp_path / "one.bin"

    async def _go() -> None:
        await transfer._download_one(  # noqa: SLF001
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
    client = FakeClient()
    state = svc._state_for(MODEL)  # noqa: SLF001
    dest = tmp_path / "aux.bin"

    async def _go() -> None:
        await transfer._download_one(  # noqa: SLF001
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
