"""``/chat/stream`` 路由集成测试的共享工具（原内联在 test_chat_stream.py）。

非 ``test_*.py`` 命名（pytest 不收集）；供 test_chat_stream 与
test_chat_stream_selfcorrect 两个测试模块共用，避免两份手抄的 HMAC 签名 /
SSE 解析 / mock 载荷。

``client`` fixture 不在此处：本仓约定各测试文件自持该 fixture（见
test_chat_retrieve_degrade.py），避免「导入 fixture」触 ruff 的 F401/F811。

请求经 HMAC 中间件签名（对齐 hmac_auth 的 canonical 构造），走真实路由层。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from fastapi.testclient import TestClient
    from httpx import Response

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.hybrid_search import FusedHit  # noqa: E402
from app.services.rerank_service import RerankResult  # noqa: E402
from app.services.rewrite_service import RewriteResult  # noqa: E402

TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"
PATH_STREAM = "/chat/stream"

PDF_PATH = "/docs/2024Q3财务报告.pdf"


def rewrite_result(query: str) -> RewriteResult:
    return RewriteResult(
        rewritten_query=query,
        need_rewrite=True,
        expanded_keywords=["2024", "Q3"],
        reason="",
    )


def sign(psk: bytes, message: str) -> str:
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def post_sse(client: TestClient, payload: dict[str, object], seq: int = 1) -> Response:
    """携带 HMAC 签名的 POST /chat/stream。"""
    body = json.dumps(payload, ensure_ascii=False)
    canonical = f"POST|{PATH_STREAM}|{body}|{seq}"
    signature = sign(TEST_PSK, canonical)
    resp: Response = client.post(
        PATH_STREAM,
        content=body,
        headers={
            "Content-Type": "application/json",
            "X-Signature": signature,
            "X-Request-Seq": str(seq),
        },
    )
    return resp


def parse_sse(text: str) -> list[tuple[str, dict[str, object]]]:
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


def vector_hits() -> list[FusedHit]:
    """两个向量命中（chunk_text 留空，模拟 LanceDB 未存文本的 FTS-only 场景）。"""
    return [
        FusedHit(chunk_id="c1", rrf_score=0.0166, file_path=PDF_PATH, chunk_text="", page=3),
        FusedHit(chunk_id="c2", rrf_score=0.0156, file_path=PDF_PATH, chunk_text="", page=5),
    ]


def payload() -> dict[str, object]:
    """标准请求体：历史 1 轮 + 两条 FTS 命中（含原文，供 FTS-only 补全）。"""
    return {
        "query": "那营收多少？",
        "history": [{"user": "2024年Q3营收是多少？", "assistant": "5.2亿元"}],
        "table_name": "documents_bge-large-zh-v1.5_v1",
        "fts_chunks": [
            {
                "chunk_id": "c1",
                "text": "2024年第三季度营收为5.2亿元，去年同期4.5亿元",
                "file_path": PDF_PATH,
                "page": 3,
            },
            {"chunk_id": "c2", "text": "营收同比增长15.6%", "file_path": PDF_PATH, "page": 5},
        ],
    }


def rerank_results() -> list[RerankResult]:
    return [
        RerankResult(chunk_id="c1", score=0.92, file_path=PDF_PATH, page=3),
        RerankResult(chunk_id="c2", score=0.88, file_path=PDF_PATH, page=5),
    ]


async def fake_citations(token_stream: object, valid_ids: set[int]) -> object:
    """引用流：两段正文 + 两个引用 id（对应 sources.id 1/2）。"""
    yield ("text", "根据财务报告，2024年营收为5.2亿元")
    yield ("citation", 1)
    yield ("text", "，同比增长15.6%")
    yield ("citation", 2)


def dummy_token_stream() -> object:
    """返回一个永不消费的空流式迭代器（fake_citations 忽略输入）。"""

    async def gen() -> object:
        yield "ignored"

    return gen()


def wrong_answer_stream() -> object:
    """P-04 初始回答流：含 [1] 引用的错误答案（营收 5.5 亿与文档 5.2 亿不符）。"""

    async def gen() -> object:
        yield "营收为5.5亿元"
        yield " [1]"

    return gen()
