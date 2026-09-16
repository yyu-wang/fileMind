"""检索降级兜底：重排模型不可用 → 融合排序 Top-K，问答不中断。

从 ``test_chat_stream.py`` 拆出（同 ``test_chat_provider_modes.py`` 的拆分理由：
rules/complexity.md 的测试文件行数阈值），并把「重排不可用如何兜底」独立为一个关注点。

回归动机：rerank 在 sidecar 进程内用 transformers ``CrossEncoder`` 本地加载
（embedding/LLM 走 Ollama，无此问题），模型文件只存在于 HuggingFace 缓存
（``~/.cache/huggingface``）。换机 / 重装系统后缓存为空、镜像又不可达时加载必失败；
旧行为是整条 /chat/stream 以 ``RERANK_UNAVAILABLE`` 中断（知识问答整体不可用），
现改为「RRF 融合序 Top-K + search_warning(RERANK_DEGRADED)」。

请求经 HMAC 中间件签名（对齐 hmac_auth 的 canonical 构造），走真实路由层。
"""

from __future__ import annotations

import contextlib
import hashlib
import hmac
import json
import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest
from fastapi.testclient import TestClient

if TYPE_CHECKING:
    from collections.abc import Generator

    from httpx import Response

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app import state  # noqa: E402
from app.main import app  # noqa: E402
from app.services.hybrid_search import FusedHit  # noqa: E402
from app.services.query_cache import reset_query_cache  # noqa: E402
from app.services.rerank_service import RerankUnavailableError  # noqa: E402
from app.services.rewrite_service import RewriteResult  # noqa: E402
from app.services.self_correct_service import SelfCorrectResult  # noqa: E402

_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"
_PATH = "/chat/stream"
_PDF = "/docs/2024Q3财务报告.pdf"
#: 降级提示（前端在 error 位展示，事件契约见 routes_chat 模块 docstring）
_DEGRADED_WARNING = {
    "code": "RERANK_DEGRADED",
    "message": "重排模型不可用，已降级为融合排序（结果相关性可能下降）",
}


@pytest.fixture
def client() -> Generator[TestClient, None, None]:
    """重置全局状态、检索缓存，注入固定 PSK、预置 LanceDB 管理器。"""
    reset_query_cache()
    state.reset_state()
    state.set_psk(_TEST_PSK)
    state.set_lancedb(object())  # 检索与重排均 mock，mgr 不被使用
    yield TestClient(app)
    reset_query_cache()


def _post_sse(client: TestClient, payload: dict[str, object], seq: int = 1) -> Response:
    """携带 HMAC 签名的 POST /chat/stream。"""
    body = json.dumps(payload, ensure_ascii=False)
    canonical = f"POST|{_PATH}|{body}|{seq}"
    signature = hmac.new(_TEST_PSK, canonical.encode("utf-8"), hashlib.sha256).hexdigest()
    resp: Response = client.post(
        _PATH,
        content=body,
        headers={
            "Content-Type": "application/json",
            "X-Signature": signature,
            "X-Request-Seq": str(seq),
        },
    )
    return resp


def _parse_sse(text: str) -> list[tuple[str, dict[str, object]]]:
    """按 ``event: ...\\ndata: ...`` 帧解析 SSE 文本。"""
    frames: list[tuple[str, dict[str, object]]] = []
    for frame in text.strip().split("\n\n"):
        if not frame:
            continue
        lines = frame.split("\n")
        event = lines[0].split(":", 1)[1].strip()
        data = json.loads(lines[1].split(":", 1)[1].strip())
        frames.append((event, data))
    return frames


def _payload() -> dict[str, object]:
    """最小请求体：两条 FTS 命中（含原文，供 FTS-only 场景回填片段的文本）。"""
    return {
        "query": "那营收多少？",
        "table_name": "documents_bge-large-zh-v1.5_v1",
        "fts_chunks": [
            {
                "chunk_id": "c1",
                "text": "2024年第三季度营收为5.2亿元，去年同期4.5亿元",
                "file_path": _PDF,
                "page": 3,
            },
            {"chunk_id": "c2", "text": "营收同比增长15.6%", "file_path": _PDF, "page": 5},
        ],
    }


def _fused_hits() -> list[FusedHit]:
    """两条融合命中，按 rrf_score 降序（hybrid_search 的输出顺序）。"""
    return [
        FusedHit(chunk_id="c1", rrf_score=0.0166, file_path=_PDF, chunk_text="", page=3),
        FusedHit(chunk_id="c2", rrf_score=0.0156, file_path=_PDF, chunk_text="", page=5),
    ]


async def _fake_answer(token_stream: object, valid_ids: set[int]) -> object:
    """生成流：单块正文（不 mock 真实 stream_with_citations 之外的逻辑）。"""
    yield ("text", "根据财务报告，2024年营收为5.2亿元")


def _dummy_token_stream() -> object:
    """永不消费的空流式迭代器（_fake_answer 忽略输入）。"""

    async def gen() -> object:
        yield "ignored"

    return gen()


def _request(client: TestClient, rerank_obj: object, seq: int = 1) -> Response:
    """在「重排给定行为 + 其余链路 mock」下发起一次 /chat/stream。"""
    with contextlib.ExitStack() as stack:
        stack.enter_context(
            mock.patch(
                "app.api.routes_chat.rewrite_query",
                new=mock.AsyncMock(
                    return_value=RewriteResult(rewritten_query="改写", need_rewrite=True)
                ),
            )
        )
        stack.enter_context(
            mock.patch(
                "app.api.routes_chat.hybrid_search",
                new=mock.AsyncMock(return_value=_fused_hits()),
            )
        )
        stack.enter_context(mock.patch("app.api.routes_chat.rerank", new=rerank_obj))
        stack.enter_context(
            mock.patch(
                "app.api.routes_chat.stream_generate", new=lambda *a, **k: _dummy_token_stream()
            )
        )
        stack.enter_context(
            mock.patch("app.api.routes_chat.stream_with_citations", new=_fake_answer)
        )
        stack.enter_context(
            mock.patch(
                "app.api.routes_chat.validate_answer",
                new=mock.AsyncMock(return_value=SelfCorrectResult(is_correct=True)),
            )
        )
        return _post_sse(client, _payload(), seq=seq)


def _rerank_unavailable() -> mock.AsyncMock:
    """重排不可用的 mock（每轮对话都触发降级路径）。"""
    return mock.AsyncMock(side_effect=RerankUnavailableError("Rerank 模型加载失败"))


def test_rerank_unavailable_degrades_to_fused_order(client: TestClient) -> None:
    """重排不可用 → search_warning(RERANK_DEGRADED) + 融合序 Top-K，问答不中断。"""
    events = _parse_sse(_request(client, _rerank_unavailable()).text)

    assert [e[0] for e in events] == [
        "search_start",
        "search_warning",
        "search_result",
        "token",
        "done",
    ]
    assert events[1][1] == _DEGRADED_WARNING
    # 降级保留 RRF 融合序取 Top-K，score 为融合分（非 cross-encoder 相关分）
    assert events[2][1]["sources"] == [
        {"id": 1, "file_name": "2024Q3财务报告.pdf", "page": 3, "score": 0.0166},
        {"id": 2, "file_name": "2024Q3财务报告.pdf", "page": 5, "score": 0.0156},
    ]


def test_rerank_degraded_result_not_cached(client: TestClient) -> None:
    """降级结果不入查询缓存 → 同问句第二次仍重试重排（模型恢复后立刻生效）。"""
    rerank_mock = _rerank_unavailable()
    first = _request(client, rerank_mock)
    second = _request(client, rerank_mock, seq=2)

    assert _parse_sse(first.text)[-1][0] == "done"
    assert _parse_sse(second.text)[-1][0] == "done"
    assert rerank_mock.await_count == 2
