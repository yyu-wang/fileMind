"""T5.6/T5.7 — /chat/stream SSE 路由集成测试（mock 服务层，不发真实推理）。

覆盖：事件顺序（search_start → search_result → token* → citation → done）与字段
对齐、FTS-only 命中文本补全、引用 id 与 sources.id 对应、无候选兜底回答、
LLM 不可用 → error 事件、向量库未初始化 → INTERNAL_ERROR、历史透传给查询改写；
T5.7 P-04 自我纠正：retry 事件重推修正答案、重试耗尽 low_confidence、
验证 LLM 故障 fail-open、无修正回答仅标记低置信度。

请求经 HMAC 中间件签名（对齐 hmac_auth 的 canonical 构造），走真实路由层。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest
from fastapi.testclient import TestClient

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app import state  # noqa: E402
from app.api.routes_chat import NOT_FOUND_ANSWER  # noqa: E402
from app.main import app  # noqa: E402
from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.hybrid_search import FusedHit  # noqa: E402
from app.services.rerank_service import RerankResult  # noqa: E402
from app.services.rewrite_service import ConversationTurn, RewriteResult  # noqa: E402
from app.services.self_correct_service import SelfCorrectResult  # noqa: E402

if TYPE_CHECKING:
    from httpx import Response

_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"
_PATH = "/chat/stream"

_PDF = "/docs/2024Q3财务报告.pdf"


def _rewrite(query: str) -> RewriteResult:
    return RewriteResult(
        rewritten_query=query,
        need_rewrite=True,
        expanded_keywords=["2024", "Q3"],
        reason="",
    )


@pytest.fixture
def client() -> TestClient:
    """重置全局状态、注入固定 PSK、预置 LanceDB 管理器。"""
    state.reset_state()
    state.set_psk(_TEST_PSK)
    state.set_lancedb(object())  # hybrid_search/rerank 均 mock，mgr 不被使用
    return TestClient(app)


def _sign(psk: bytes, message: str) -> str:
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _post_sse(client: TestClient, payload: dict[str, object], seq: int = 1) -> Response:
    """携带 HMAC 签名的 POST /chat/stream。"""
    body = json.dumps(payload, ensure_ascii=False)
    canonical = f"POST|{_PATH}|{body}|{seq}"
    signature = _sign(_TEST_PSK, canonical)
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


def _vector_hits() -> list[FusedHit]:
    """两个向量命中（chunk_text 留空，模拟 LanceDB 未存文本的 FTS-only 场景）。"""
    return [
        FusedHit(chunk_id="c1", rrf_score=0.0166, file_path=_PDF, chunk_text="", page=3),
        FusedHit(chunk_id="c2", rrf_score=0.0156, file_path=_PDF, chunk_text="", page=5),
    ]


def _payload() -> dict[str, object]:
    """标准请求体：历史 1 轮 + 两条 FTS 命中（含原文，供 FTS-only 补全）。"""
    return {
        "query": "那营收多少？",
        "history": [{"user": "2024年Q3营收是多少？", "assistant": "5.2亿元"}],
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


def _rerank_results() -> list[RerankResult]:
    return [
        RerankResult(chunk_id="c1", score=0.92, file_path=_PDF, page=3),
        RerankResult(chunk_id="c2", score=0.88, file_path=_PDF, page=5),
    ]


async def _fake_citations(token_stream: object, valid_ids: set[int]) -> object:
    """引用流：两段正文 + 两个引用 id（对应 sources.id 1/2）。"""
    yield ("text", "根据财务报告，2024年营收为5.2亿元")
    yield ("citation", 1)
    yield ("text", "，同比增长15.6%")
    yield ("citation", 2)


def _dummy_token_stream() -> object:
    """返回一个永不消费的空流式迭代器（fake_citations 忽略输入）。"""

    async def gen() -> object:
        yield "ignored"

    return gen()


def _wrong_answer_stream() -> object:
    """P-04 初始回答流：含 [1] 引用的错误答案（营收 5.5 亿与文档 5.2 亿不符）。"""

    async def gen() -> object:
        yield "营收为5.5亿元"
        yield " [1]"

    return gen()


def test_sse_event_sequence_and_fields(client: TestClient) -> None:
    """完整流水线 → search_start → search_result → token* → citation → done，字段对齐契约。"""
    rewritten = "2024年Q3营收是多少"
    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite(rewritten)),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate", new=lambda *a, **k: _dummy_token_stream()
        ),
        mock.patch("app.api.routes_chat.stream_with_citations", new=_fake_citations),
        mock.patch(
            "app.api.routes_chat.validate_answer",
            new=mock.AsyncMock(return_value=SelfCorrectResult(is_correct=True)),
        ),
    ):
        resp = _post_sse(client, _payload())

    assert resp.status_code == 200
    assert resp.headers["content-type"].startswith("text/event-stream")
    events = _parse_sse(resp.text)

    assert events[0][0] == "search_start"
    assert events[0][1] == {"query_original": "那营收多少？", "query_rewritten": rewritten}

    assert events[1][0] == "search_result"
    result = events[1][1]
    assert result["candidates"] == 2
    assert result["after_rerank"] == 2
    assert result["sources"] == [
        {"id": 1, "file_name": "2024Q3财务报告.pdf", "page": 3, "score": 0.92},
        {"id": 2, "file_name": "2024Q3财务报告.pdf", "page": 5, "score": 0.88},
    ]

    assert events[2][0] == "token"
    assert events[2][1]["content"] == "根据财务报告，2024年营收为5.2亿元"
    assert events[3][0] == "token"
    assert events[3][1]["content"] == "，同比增长15.6%"

    assert events[4][0] == "citation"
    citations = events[4][1]["citations"]
    assert citations == [
        {
            "id": 1,
            "file_name": "2024Q3财务报告.pdf",
            "page": 3,
            "text": "2024年第三季度营收为5.2亿元，去年同期4.5亿元",
        },
        {"id": 2, "file_name": "2024Q3财务报告.pdf", "page": 5, "text": "营收同比增长15.6%"},
    ]
    # IT-005：citation.id 均在 search_result.sources.id 中
    source_ids = {s["id"] for s in result["sources"]}
    assert {c["id"] for c in citations} <= source_ids

    assert events[5][0] == "done"
    done = events[5][1]
    assert done["session_id"]
    assert done["total_tokens"] == 2
    assert done["duration_ms"] >= 0
    assert "low_confidence" not in done


def test_history_passed_to_rewrite(client: TestClient) -> None:
    """请求 history → 转为 ConversationTurn 传给 rewrite_query（P-02 输入）。"""
    rewrite_mock = mock.AsyncMock(return_value=_rewrite("改写"))
    with (
        mock.patch("app.api.routes_chat.rewrite_query", new=rewrite_mock),
        mock.patch("app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=[])),
        mock.patch("app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=[])),
    ):
        _post_sse(client, _payload())

    rewrite_mock.assert_awaited_once_with(
        "那营收多少？",
        [ConversationTurn(user="2024年Q3营收是多少？", assistant="5.2亿元")],
        provider=None,
    )


def test_no_candidates_short_circuit(client: TestClient) -> None:
    """无检索候选 → token 兜底回答「根据现有文档，未找到相关信息」+ done，无 citation。"""
    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("改写")),
        ),
        mock.patch("app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=[])),
        mock.patch("app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=[])),
    ):
        resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "token", "done"]
    assert events[2][1]["content"] == NOT_FOUND_ANSWER


def test_llm_unavailable_yields_error_event(client: TestClient) -> None:
    """生成阶段 Ollama 不可用 → 检索事件后 error 事件 OLLAMA_UNAVAILABLE。"""

    def boom(*args: object, **kwargs: object) -> object:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("改写")),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate", new=lambda *a, **k: _dummy_token_stream()
        ),
        mock.patch("app.api.routes_chat.stream_with_citations", new=boom),
    ):
        resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    # 检索成功（search_start/search_result 先发），生成阶段失败 → error 结尾
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "error"]
    assert events[-1] == ("error", {"code": "OLLAMA_UNAVAILABLE", "message": "Ollama down"})


def test_lancedb_uninitialized_yields_internal_error(client: TestClient) -> None:
    """向量库未初始化（state.get_lancedb() 为 None）→ error INTERNAL_ERROR。"""
    state.set_lancedb(None)
    resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    assert events == [("error", {"code": "INTERNAL_ERROR", "message": "向量库未初始化"})]


# ------------------------------------------------------------------
# T5.7 P-04 自我纠正：retry 重推修正答案 / 重试耗尽 / fail-open / 无修正
# ------------------------------------------------------------------


def test_retry_replays_corrected_answer(client: TestClient) -> None:
    """P-04 检出问题 → retry 事件后重推修正答案 token 流 → citation → done。

    不 mock ``stream_with_citations``——用真实解析验证修正答案的重推逻辑。
    """
    corrected = "修正后的回答 [1]"
    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate",
            new=lambda *a, **k: _wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.routes_chat.validate_answer",
            new=mock.AsyncMock(
                side_effect=[
                    SelfCorrectResult(
                        is_correct=False,
                        issues=["数据错误：营收应为5.2亿"],
                        corrected_answer=corrected,
                        reason="数据错误：营收应为5.2亿",
                    ),
                    SelfCorrectResult(is_correct=True),
                ]
            ),
        ),
    ):
        resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    names = [e[0] for e in events]
    assert names == [
        "search_start",
        "search_result",
        "token",
        "retry",
        "token",
        "citation",
        "done",
    ]
    assert events[2][1] == {"content": "营收为5.5亿元 "}
    assert events[3][0] == "retry"
    assert events[3][1] == {
        "reason": "数据错误：营收应为5.2亿",
        "attempt": 1,
        "rewritten_query": "2024年Q3营收多少",
    }
    # 修正答案重推为单块 token，真实 stream_with_citations 解析出正文 + 引用
    assert events[4][1] == {"content": "修正后的回答 "}
    assert events[5][1]["citations"] == [
        {
            "id": 1,
            "file_name": "2024Q3财务报告.pdf",
            "page": 3,
            "text": "2024年第三季度营收为5.2亿元，去年同期4.5亿元",
        }
    ]
    assert events[6][0] == "done"
    assert events[6][1]["total_tokens"] == 2
    assert "low_confidence" not in events[6][1]


def test_retry_exhausted_marks_low_confidence(client: TestClient) -> None:
    """重试耗尽（max_retries=2，连续 3 次验证失败）→ 2 次 retry + done.low_confidence。"""
    incorrect = SelfCorrectResult(
        is_correct=False, issues=["问题"], corrected_answer="修正 [1]", reason="问题"
    )
    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate",
            new=lambda *a, **k: _wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.routes_chat.validate_answer",
            new=mock.AsyncMock(side_effect=[incorrect, incorrect, incorrect]),
        ),
    ):
        resp = _post_sse(client, {**_payload(), "max_retries": 2})

    events = _parse_sse(resp.text)
    names = [e[0] for e in events]
    # search_start search_result token retry token retry token citation done
    assert names.count("retry") == 2
    assert events[3][1]["attempt"] == 1
    assert events[5][1]["attempt"] == 2
    assert events[-1][0] == "done"
    assert events[-1][1]["low_confidence"] is True


def test_validate_llm_unavailable_fail_open(client: TestClient) -> None:
    """验证阶段 Ollama 故障 → fail-open：不重试，直接 done（无 low_confidence）。"""

    async def boom(query: str, context: str, answer: str, provider=None) -> object:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate",
            new=lambda *a, **k: _wrong_answer_stream(),
        ),
        mock.patch("app.api.routes_chat.validate_answer", new=boom),
    ):
        resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "token", "citation", "done"]
    assert "low_confidence" not in events[-1][1]


def test_corrected_empty_marks_low_confidence(client: TestClient) -> None:
    """P-04 检出问题但无修正回答 → 不重试，直接 done.low_confidence=true。"""
    with (
        mock.patch(
            "app.api.routes_chat.rewrite_query",
            new=mock.AsyncMock(return_value=_rewrite("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.routes_chat.hybrid_search", new=mock.AsyncMock(return_value=_vector_hits())
        ),
        mock.patch(
            "app.api.routes_chat.rerank", new=mock.AsyncMock(return_value=_rerank_results())
        ),
        mock.patch(
            "app.api.routes_chat.stream_generate",
            new=lambda *a, **k: _wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.routes_chat.validate_answer",
            new=mock.AsyncMock(
                return_value=SelfCorrectResult(
                    is_correct=False, issues=["无引用"], corrected_answer=None, reason="无引用"
                )
            ),
        ),
    ):
        resp = _post_sse(client, _payload())

    events = _parse_sse(resp.text)
    names = [e[0] for e in events]
    assert "retry" not in names
    assert names == ["search_start", "search_result", "token", "citation", "done"]
    assert events[-1][1]["low_confidence"] is True
