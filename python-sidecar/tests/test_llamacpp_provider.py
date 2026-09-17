"""T3b — LlamaCppProvider（内置 llama.cpp 引擎的 Provider）单元测试。

覆盖：请求体构造（含 ``response_format`` JSON 约束）、响应/SSE 解析的容错边界、
HTTP 与引擎不可用两条失败路径的错误映射、以及 ``embed`` 的显式不支持。

HTTP 层用替身客户端（不触网、跨平台）。真实引擎的端到端验证在
``test_local_llm_service.py`` 的桩引擎用例与 T3 的真机验证中完成。
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import httpx
import pytest

if TYPE_CHECKING:
    from collections.abc import Iterator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services import local_llm_service  # noqa: E402
from app.services.embedding_service import EmbeddingUnavailableError  # noqa: E402
from app.services.providers.llamacpp_provider import (  # noqa: E402
    LlamaCppProvider,
    build_payload,
    delta_of_sse_line,
    parse_content,
)

_BASE_URL = "http://127.0.0.1:51234"


def _patch_engine(monkeypatch: pytest.MonkeyPatch, *, base_url: str = _BASE_URL) -> None:
    """让 ensure_server 直接返回给定 base URL（不真的拉进程）。"""

    async def _fake_ensure(model: str | None = None) -> str:
        return base_url

    monkeypatch.setattr(local_llm_service, "ensure_server", _fake_ensure)


def _response(payload: object, status_code: int = 200) -> object:
    """构造非流式响应替身。"""

    class _Resp:
        def __init__(self) -> None:
            self.status_code = status_code

        def raise_for_status(self) -> None:
            if status_code >= 400:
                raise httpx.HTTPStatusError(
                    f"status {status_code}",
                    request=httpx.Request("POST", _BASE_URL),
                    response=httpx.Response(status_code, request=httpx.Request("GET", _BASE_URL)),
                )

        def json(self) -> object:
            return payload

    return _Resp()


def _patch_http(
    monkeypatch: pytest.MonkeyPatch,
    *,
    post_response: object = None,
    post_error: Exception | None = None,
    stream_lines: tuple[str, ...] = (),
    stream_error: Exception | None = None,
    stream_delay: float = 0.0,
) -> tuple[list[dict[str, object]], list[dict[str, str]]]:
    """替换 ``httpx.AsyncClient``，返回（请求体列表, 请求头列表）。"""
    recorded: list[dict[str, object]] = []
    headers_seen: list[dict[str, str]] = []

    class _Stream:
        def __init__(self) -> None:
            self.status_code = 200

        def raise_for_status(self) -> None:
            return None

        async def aiter_lines(self) -> Iterator[str]:
            for line in stream_lines:
                if stream_delay > 0:
                    await asyncio.sleep(stream_delay)
                yield line

        async def __aenter__(self) -> _Stream:
            return self

        async def __aexit__(self, *exc: object) -> bool:
            return False

    class _Client:
        def __init__(self, **kwargs: object) -> None:
            return None

        async def __aenter__(self) -> _Client:
            return self

        async def __aexit__(self, *exc: object) -> bool:
            return False

        async def post(
            self,
            url: str,
            json: dict[str, object] | None = None,
            headers: dict[str, str] | None = None,
        ) -> object:
            if json is not None:
                recorded.append(json)
            headers_seen.append(headers or {})
            if post_error is not None:
                raise post_error
            return post_response

        def stream(
            self,
            method: str,
            url: str,
            json: dict[str, object] | None = None,
            headers: dict[str, str] | None = None,
        ) -> _Stream:
            if json is not None:
                recorded.append(json)
            headers_seen.append(headers or {})
            if stream_error is not None:
                raise stream_error
            return _Stream()

    monkeypatch.setattr(httpx, "AsyncClient", _Client)
    return recorded, headers_seen


# ------------------------------------------------------------------
# 纯函数
# ------------------------------------------------------------------


def test_build_payload_minimal() -> None:
    """基础请求体：system + user + 温度 + stream，不下发未指定的 max_tokens。"""
    payload = build_payload("sys", "usr", 0.2, None, stream=False)

    assert payload["messages"] == [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "usr"},
    ]
    assert payload["temperature"] == 0.2
    assert payload["stream"] is False
    assert "max_tokens" not in payload
    assert "response_format" not in payload


def test_build_payload_json_mode_and_max_tokens() -> None:
    """json_mode 映射 response_format（P-01/P-02 依赖）；max_tokens 显式下发。"""
    payload = build_payload("s", "u", 0.0, 256, stream=False, json_mode=True)

    assert payload["response_format"] == {"type": "json_object"}
    assert payload["max_tokens"] == 256
    assert payload["stream"] is False


def test_parse_content_reads_first_choice() -> None:
    """正常响应取首个 choice 的正文。"""
    body = {"choices": [{"message": {"role": "assistant", "content": "你好"}}]}
    assert parse_content(body) == "你好"


@pytest.mark.parametrize(
    "body",
    [
        None,
        "text",
        {},
        {"choices": []},
        {"choices": [{}]},
        {"choices": [{"message": {}}]},
        {"choices": [{"message": {"content": None}}]},
    ],
)
def test_parse_content_tolerates_malformed(body: object) -> None:
    """结构异常一律返回空串（调用方有既有降级路径），不抛异常。"""
    assert parse_content(body) == ""


def test_delta_of_sse_line_extracts_content() -> None:
    """data 行取 delta 正文。"""
    line = 'data: {"choices":[{"delta":{"content":"增量"}}]}'
    assert delta_of_sse_line(line) == "增量"


@pytest.mark.parametrize(
    "line",
    [
        "",
        ": keep-alive",
        "event: message",
        "data: [DONE]",
        "data: ",
        "data: {not json}",
        'data: {"choices":[]}',
        'data: {"choices":[{"delta":{}}]}',
        'data: {"choices":[{"delta":{"content":null}}]}',
    ],
)
def test_delta_of_sse_line_ignores_non_content_lines(line: str) -> None:
    """非 data 行 / [DONE] / 畸形 / 空 delta 一律忽略。"""
    assert delta_of_sse_line(line) == ""


# ------------------------------------------------------------------
# Provider：补全
# ------------------------------------------------------------------


async def test_generate_returns_content_and_sends_json_mode(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """非流式补全：返回正文，且 json_mode 出现在请求体里。"""
    _patch_engine(monkeypatch)
    recorded, _headers = _patch_http(
        monkeypatch,
        post_response=_response({"choices": [{"message": {"content": '{"a":1}'}}]}),
    )

    result = await LlamaCppProvider().generate("sys", "usr", json_mode=True, max_tokens=64)

    assert result == '{"a":1}'
    assert recorded[0]["response_format"] == {"type": "json_object"}  # type: ignore[index]
    assert recorded[0]["max_tokens"] == 64  # type: ignore[index]


async def test_generate_maps_http_error_to_llm_unavailable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """HTTP 失败 → LLMUnavailableError（调用方按「生成不可用」降级，无需区分后端）。"""
    _patch_engine(monkeypatch)
    _patch_http(monkeypatch, post_error=httpx.ConnectError("connection refused"))

    with pytest.raises(LLMUnavailableError, match="调用失败"):
        await LlamaCppProvider().generate("s", "u")


async def test_generate_maps_engine_unavailable(monkeypatch: pytest.MonkeyPatch) -> None:
    """引擎起不来 → LLMUnavailableError，且错误里保留原因（含设置页指引）。"""

    async def _boom(model: str | None = None) -> str:
        raise local_llm_service.LocalLlmUnavailableError("权重未下载")

    monkeypatch.setattr(local_llm_service, "ensure_server", _boom)

    with pytest.raises(LLMUnavailableError, match="权重未下载"):
        await LlamaCppProvider().generate("s", "u")


async def test_generate_stream_yields_deltas_until_done(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """流式：逐个 delta yield，遇到 [DONE] 与空行自然结束。"""
    _patch_engine(monkeypatch)
    _patch_http(
        monkeypatch,
        stream_lines=(
            'data: {"choices":[{"delta":{"content":"你"}}]}',
            ": keep-alive",
            'data: {"choices":[{"delta":{"content":"好"}}]}',
            "",
            "data: [DONE]",
        ),
    )

    chunks = [chunk async for chunk in LlamaCppProvider().generate_stream("s", "u")]

    assert chunks == ["你", "好"]


async def test_generate_stream_maps_http_error(monkeypatch: pytest.MonkeyPatch) -> None:
    """流式首包前失败 → LLMUnavailableError。"""
    _patch_engine(monkeypatch)
    _patch_http(monkeypatch, stream_error=httpx.ReadTimeout("timed out"))

    with pytest.raises(LLMUnavailableError, match="流式生成失败"):
        _ = [chunk async for chunk in LlamaCppProvider().generate_stream("s", "u")]


async def test_generate_and_stream_send_engine_api_key(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """两条路径都必须带引擎鉴权头：引擎的推理端点无 key 会 401（实测）。"""
    _patch_engine(monkeypatch)
    monkeypatch.setattr(local_llm_service, "auth_headers", lambda: {"Authorization": "Bearer k1"})
    _recorded, headers = _patch_http(
        monkeypatch,
        post_response=_response({"choices": [{"message": {"content": "x"}}]}),
        stream_lines=('data: {"choices":[{"delta":{"content":"x"}}]}',),
    )

    await LlamaCppProvider().generate("s", "u")
    _ = [chunk async for chunk in LlamaCppProvider().generate_stream("s", "u")]

    assert headers[0]["Authorization"] == "Bearer k1"
    assert headers[1]["Authorization"] == "Bearer k1"


async def test_embed_is_explicitly_unsupported() -> None:
    """embed 显式不支持（向量化统一走进程内 ONNX，两个后端的向量不可混用）。"""
    with pytest.raises(EmbeddingUnavailableError, match="不提供向量化"):
        await LlamaCppProvider().embed(["文本"])


def test_version_is_local_prompt() -> None:
    """本地后端沿用本地 prompt 变体（详细指令 + 多 few-shot）。"""
    assert LlamaCppProvider().version == "local"


async def test_generate_stream_uses_total_timeout(monkeypatch: pytest.MonkeyPatch) -> None:
    """流式总量超时 → LLMUnavailableError（引擎卡死时不无限挂起）。"""
    _patch_engine(monkeypatch)
    _patch_http(
        monkeypatch,
        stream_lines=('data: {"choices":[{"delta":{"content":"x"}}]}',) * 3,
        stream_delay=0.05,
    )
    monkeypatch.setattr("app.services.providers.llamacpp_provider.STREAM_TIMEOUT", 0.01)

    with pytest.raises(LLMUnavailableError, match="流式超时"):
        _ = [chunk async for chunk in LlamaCppProvider().generate_stream("s", "u")]
