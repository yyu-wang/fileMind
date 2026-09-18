"""``model_download_service`` 测试的共享假件（原内联在 test_model_download_service.py）。

非 ``test_*.py`` 命名（pytest 不收集）；供 ``test_model_download_service``（清单 / 状态 /
下载编排）与 ``test_model_download_transfer``（Content-Range 解析 / TLS 上限 / 单文件原子写）
共用，避免两份手抄的假 httpx 客户端。

全部用例用假 httpx 客户端（按 URL 前缀模拟各镜像的行为），不发起真实网络请求。
``_isolate`` 夹具不在此处：本仓约定各测试文件自持该夹具，避免「导入 fixture」触
ruff 的 F401/F811（同 chat_stream_support.py / local_llm_support.py）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING

import httpx

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

    import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import model_download_service as svc  # noqa: E402

MODEL = "bge-large-zh-v1.5"
CONTENT = b"x" * 512


class FakeResponse:
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


class FakeStream:
    """``client.stream(...)`` 返回的异步上下文管理器。"""

    def __init__(self, response: FakeResponse) -> None:
        self._response = response

    async def __aenter__(self) -> FakeResponse:
        return self._response

    async def __aexit__(self, *exc: object) -> bool:
        return False


class FakeClient:
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

    async def __aenter__(self) -> FakeClient:
        return self

    async def __aexit__(self, *exc: object) -> bool:
        return False

    def _fails(self, url: str) -> bool:
        return any(url.startswith(mirror) for mirror in self._fail_mirrors)

    async def head(self, url: str) -> FakeResponse:
        """HEAD：失败镜像抛连接错误；否则返回（可选缺失的）Content-Length。"""
        self.head_calls.append(url)
        if self._fails(url):
            raise httpx.ConnectError("mirror down")
        headers = {} if self._head_without_length else {"content-length": str(len(CONTENT))}
        return FakeResponse([], headers)

    async def get(self, url: str, headers: dict[str, str] | None = None) -> FakeResponse:
        """Range 探测：HEAD 缺 Content-Length 时的兜底路径。"""
        self.range_calls.append(url)
        if self._fails(url):
            raise httpx.ConnectError("mirror down")
        return FakeResponse([], {"content-range": f"bytes 0-0/{len(CONTENT)}"})

    def stream(self, method: str, url: str) -> FakeStream:
        """GET 流：失败镜像返回 503（raise_for_status 抛错）。"""
        self.stream_calls.append(url)
        if self._fails(url):
            return FakeStream(FakeResponse([], {}, status=503))
        return FakeStream(FakeResponse([CONTENT], {}))


def patch_client(monkeypatch: pytest.MonkeyPatch, client: FakeClient) -> None:
    """把服务模块内的 httpx 换成只含 AsyncClient 的替身（保留异常类型）。"""
    monkeypatch.setattr(
        svc,
        "httpx",
        SimpleNamespace(
            HTTPError=httpx.HTTPError,
            AsyncClient=lambda **kwargs: client,
        ),
    )
