"""T5.6 — /chat/stream SSE 路由集成测试（mock 服务层，不发真实推理）。

覆盖：事件顺序（search_start → search_result → token* → citation → done）与字段
对齐、FTS-only 命中文本补全、引用 id 与 sources.id 对应、无候选兜底回答、
LLM 不可用 → error 事件、向量库未初始化 → INTERNAL_ERROR、历史透传给查询改写。

T5.7 P-04 自我纠正（retry / low_confidence / fail-open）与检索缓存见
``test_chat_stream_selfcorrect.py``；共享夹具与工具见 ``chat_stream_support.py``。

请求经 HMAC 中间件签名（对齐 hmac_auth 的 canonical 构造），走真实路由层。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest
from fastapi.testclient import TestClient

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app import state  # noqa: E402
from app.api.routes_chat import NOT_FOUND_ANSWER  # noqa: E402
from app.main import app  # noqa: E402
from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.query_cache import reset_query_cache  # noqa: E402
from app.services.rewrite_service import ConversationTurn  # noqa: E402
from app.services.self_correct_service import SelfCorrectResult  # noqa: E402
from tests.chat_stream_support import (  # noqa: E402
    TEST_PSK,
    dummy_token_stream,
    fake_citations,
    parse_sse,
    payload,
    post_sse,
    rerank_results,
    rewrite_result,
    vector_hits,
)


@pytest.fixture
def client() -> Generator[TestClient, None, None]:
    """重置全局状态、检索缓存，注入固定 PSK、预置 LanceDB 管理器。"""
    reset_query_cache()
    state.reset_state()
    state.set_psk(TEST_PSK)
    state.set_lancedb(object())  # hybrid_search/rerank 均 mock，mgr 不被使用
    yield TestClient(app)
    reset_query_cache()


def test_sse_event_sequence_and_fields(client: TestClient) -> None:
    """完整流水线 → search_start → search_result → token* → citation → done，字段对齐契约。"""
    rewritten = "2024年Q3营收是多少"
    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result(rewritten)),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch("app.api.chat_answer.stream_generate", new=lambda *a, **k: dummy_token_stream()),
        mock.patch("app.api.chat_answer.stream_with_citations", new=fake_citations),
        mock.patch(
            "app.api.chat_answer.validate_answer",
            new=mock.AsyncMock(return_value=SelfCorrectResult(is_correct=True)),
        ),
    ):
        resp = post_sse(client, payload())

    assert resp.status_code == 200
    assert resp.headers["content-type"].startswith("text/event-stream")
    events = parse_sse(resp.text)

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
    # SC-m19：total_tokens 按 len(text)//4 估算（5+2=7）
    assert done["total_tokens"] == 7
    assert done["duration_ms"] >= 0
    # T10.2 TTFT 指标：retrieve 耗时 + 首 token 耗时（≤ 总时长）
    assert done["retrieve_ms"] >= 0
    assert done["first_token_ms"] >= 0
    assert done["first_token_ms"] <= done["duration_ms"]
    assert "low_confidence" not in done


def test_history_passed_torewrite_result(client: TestClient) -> None:
    """请求 history → 转为 ConversationTurn 传给 rewrite_query（P-02 输入）。"""
    rewrite_mock = mock.AsyncMock(return_value=rewrite_result("改写"))
    with (
        mock.patch("app.api.chat_retrieve.rewrite_query", new=rewrite_mock),
        mock.patch("app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=[])),
        mock.patch("app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=[])),
    ):
        post_sse(client, payload())

    # SC-m9：rewrite_query 新增 model 参数（默认 LLM_MODEL）
    rewrite_mock.assert_awaited_once_with(
        "那营收多少？",
        [ConversationTurn(user="2024年Q3营收是多少？", assistant="5.2亿元")],
        provider=None,
        model="qwen3.8-27b",
    )


def test_no_candidates_short_circuit(client: TestClient) -> None:
    """无检索候选 → token 兜底回答「根据现有文档，未找到相关信息」+ done，无 citation。"""
    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("改写")),
        ),
        mock.patch("app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=[])),
        mock.patch("app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=[])),
    ):
        resp = post_sse(client, payload())

    events = parse_sse(resp.text)
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "token", "done"]
    assert events[2][1]["content"] == NOT_FOUND_ANSWER


def test_llm_unavailable_yields_error_event(client: TestClient) -> None:
    """生成阶段 Ollama 不可用 → 检索事件后 error 事件 LLM_UNAVAILABLE（SC-m10）。"""

    def boom(*args: object, **kwargs: object) -> object:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("改写")),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch("app.api.chat_answer.stream_generate", new=lambda *a, **k: dummy_token_stream()),
        mock.patch("app.api.chat_answer.stream_with_citations", new=boom),
    ):
        resp = post_sse(client, payload())

    events = parse_sse(resp.text)
    # 检索成功（search_start/search_result 先发），生成阶段失败 → error 结尾
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "error"]
    # SC-m10：错误码从 OLLAMA_UNAVAILABLE 改为 LLM_UNAVAILABLE
    assert events[-1] == ("error", {"code": "LLM_UNAVAILABLE", "message": "Ollama down"})


def test_lancedb_uninitialized_yields_internal_error(client: TestClient) -> None:
    """向量库未初始化（state.get_lancedb() 为 None）→ error INTERNAL_ERROR。"""
    state.set_lancedb(None)
    resp = post_sse(client, payload())

    events = parse_sse(resp.text)
    assert events == [("error", {"code": "INTERNAL_ERROR", "message": "向量库未初始化"})]


def test_unexpected_exception_yields_error_event(client: TestClient) -> None:
    """检索阶段抛意外异常（如 LanceDB 表不存在）→ 流以 error 事件收尾（SC-M2 回归）。

    旧实现只捕获三类 UnavailableError，意外异常使 async generator 中途崩溃，
    SSE 连接断开且无 error 事件；修复后兜底产出 INTERNAL_ERROR 后正常收尾，
    且异常细节不透给前端（只进服务端日志）。
    """

    async def boom(*args: object, **kwargs: object) -> object:
        raise RuntimeError("table documents_xxx_v1 not found")

    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("改写")),
        ),
        mock.patch("app.api.chat_retrieve.hybrid_search", new=boom),
    ):
        resp = post_sse(client, payload())

    assert resp.status_code == 200
    events = parse_sse(resp.text)
    assert events == [("error", {"code": "INTERNAL_ERROR", "message": "生成回答时发生内部错误"})]
