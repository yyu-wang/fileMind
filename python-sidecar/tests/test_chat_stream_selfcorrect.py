"""T5.7 — /chat/stream 的 P-04 自我纠正与检索缓存（mock 服务层）。

覆盖：retry 事件重推修正答案、重试耗尽 low_confidence、验证 LLM 故障 fail-open、
无修正回答仅标记低置信度；以及 T10.2 查询缓存命中时跳过整条检索管线。

事件契约（search_start → search_result → token* → citation → done）与字段对齐见
``test_chat_stream.py``；共享夹具与工具见 ``chat_stream_support.py``。

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
from app.main import app  # noqa: E402
from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.query_cache import reset_query_cache  # noqa: E402
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
    wrong_answer_stream,
)


@pytest.fixture
def client() -> Generator[TestClient, None, None]:
    """重置全局状态、检索缓存，注入固定 PSK、预置 LanceDB 管理器。"""
    reset_query_cache()
    state.reset_state()
    state.set_psk(TEST_PSK)
    state.set_lancedb(object())  # 检索与重排均 mock，mgr 不被使用
    yield TestClient(app)
    reset_query_cache()


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
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch(
            "app.api.chat_answer.stream_generate",
            new=lambda *a, **k: wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.chat_answer.validate_answer",
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
        resp = post_sse(client, payload())

    events = parse_sse(resp.text)
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
    # SC-m19：total_tokens 按 len(text)//4 估算（1+1+1=3）
    assert events[6][1]["total_tokens"] == 3
    assert "low_confidence" not in events[6][1]


def test_retry_exhausted_marks_low_confidence(client: TestClient) -> None:
    """重试耗尽（max_retries=2，连续 3 次验证失败）→ 2 次 retry + done.low_confidence。"""
    incorrect = SelfCorrectResult(
        is_correct=False, issues=["问题"], corrected_answer="修正 [1]", reason="问题"
    )
    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch(
            "app.api.chat_answer.stream_generate",
            new=lambda *a, **k: wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.chat_answer.validate_answer",
            new=mock.AsyncMock(side_effect=[incorrect, incorrect, incorrect]),
        ),
    ):
        resp = post_sse(client, {**payload(), "max_retries": 2})

    events = parse_sse(resp.text)
    names = [e[0] for e in events]
    # search_start search_result token retry token retry token citation done
    assert names.count("retry") == 2
    assert events[3][1]["attempt"] == 1
    assert events[5][1]["attempt"] == 2
    assert events[-1][0] == "done"
    assert events[-1][1]["low_confidence"] is True


def test_validate_llm_unavailable_fail_open(client: TestClient) -> None:
    """验证阶段 Ollama 故障 → fail-open：不重试，直接 done（无 low_confidence）。"""

    # SC-m9：validate_answer 新增 model 参数
    async def boom(
        query: str, context: str, answer: str, provider=None, **kwargs: object
    ) -> object:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch(
            "app.api.chat_answer.stream_generate",
            new=lambda *a, **k: wrong_answer_stream(),
        ),
        mock.patch("app.api.chat_answer.validate_answer", new=boom),
    ):
        resp = post_sse(client, payload())

    events = parse_sse(resp.text)
    names = [e[0] for e in events]
    assert names == ["search_start", "search_result", "token", "citation", "done"]
    assert "low_confidence" not in events[-1][1]


def test_corrected_empty_marks_low_confidence(client: TestClient) -> None:
    """P-04 检出问题但无修正回答 → 不重试，直接 done.low_confidence=true。"""
    with (
        mock.patch(
            "app.api.chat_retrieve.rewrite_query",
            new=mock.AsyncMock(return_value=rewrite_result("2024年Q3营收多少")),
        ),
        mock.patch(
            "app.api.chat_retrieve.hybrid_search", new=mock.AsyncMock(return_value=vector_hits())
        ),
        mock.patch(
            "app.api.chat_retrieve.rerank", new=mock.AsyncMock(return_value=rerank_results())
        ),
        mock.patch(
            "app.api.chat_answer.stream_generate",
            new=lambda *a, **k: wrong_answer_stream(),
        ),
        mock.patch(
            "app.api.chat_answer.validate_answer",
            new=mock.AsyncMock(
                return_value=SelfCorrectResult(
                    is_correct=False, issues=["无引用"], corrected_answer=None, reason="无引用"
                )
            ),
        ),
    ):
        resp = post_sse(client, payload())

    events = parse_sse(resp.text)
    names = [e[0] for e in events]
    assert "retry" not in names
    assert names == ["search_start", "search_result", "token", "citation", "done"]
    assert events[-1][1]["low_confidence"] is True


def test_query_cache_hit_skips_retrieval_pipeline(client: TestClient) -> None:
    """相同请求第二次命中缓存 → 改写/混合检索/重排只 await 一次；done 字段仍齐全。"""
    rewritten = "2024年Q3营收是多少"
    rewrite_mock = mock.AsyncMock(return_value=rewrite_result(rewritten))
    with (
        mock.patch("app.api.chat_retrieve.rewrite_query", new=rewrite_mock),
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
        first = post_sse(client, payload())
        # 防重放：序号递增；两条请求仅 fts_chunks/query 一致（命中缓存的前提）
        second = post_sse(client, payload(), seq=2)

    first_done = parse_sse(first.text)[-1]
    second_done = parse_sse(second.text)[-1]
    assert first_done[0] == "done"
    assert second_done[0] == "done"
    # 第二次命中缓存：不再走检索管线（改写仅 await 一次），但 done 指标仍在
    assert rewrite_mock.await_count == 1
    assert second_done[1]["retrieve_ms"] >= 0
    assert second_done[1]["first_token_ms"] >= 0
