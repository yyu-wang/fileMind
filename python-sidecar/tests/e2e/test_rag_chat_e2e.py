"""T5.8 — E2E-003 RAG 问答完整流程（sidecar 层 API E2E，真实 Ollama + LanceDB）。

用真实后端驱动 ``/chat/stream`` 全链路（改写 → 混合检索 → 重排 → 流式生成 →
自我纠正），验证 E2E-003 在 sidecar 端可验证的语义：
  1. 流式输出：SSE 多个 ``token`` 帧增量输出（非一次性整块）
  2. 引用跳转数据：``citation`` 事件含 ``file_name`` + ``page``（前端「引用标签
     点击 → 文件预览定位页码」的依据），且 id ⊆ ``sources.id``
  3. 回答内容：首答含「本地推理」「云端推理」（与检索文档一致）
  4. 多轮对话 + 代词消解：带 history 的追问命中「隐私」文档

默认跳过（需真实 Ollama：qwen3.8-27b / bge-large-zh-v1.5 / bge-reranker-v2-m3）：
    RUN_E2E=1 .venv/bin/pytest -m e2e python-sidecar/tests/e2e/ -q
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # noqa: E402

from fastapi.testclient import TestClient  # noqa: E402

from app import state  # noqa: E402
from app.db.lancedb_repo import LanceDBManager  # noqa: E402
from app.main import app  # noqa: E402
from app.services.embedding_service import embed_texts  # noqa: E402

_PSK: bytes = b"e2e-test-psk-32-bytes-need-32-bytes!!"
_TABLE = "documents_bge-large-zh-v1.5_v1"
_PATH = "/chat/stream"

#: 确定性语料：主题「推理模式」，两轮查询均命中目标（E2E-003 示例问题）。
_CHUNKS: list[dict[str, object]] = [
    {
        "chunk_id": "c1",
        "text": "FileMind 支持两种推理模式：本地推理和云端推理。"
        "本地推理使用本机 Ollama 模型，云端推理通过远程 API。",
        "file_path": "/docs/使用说明.md",
        "page": 2,
    },
    {
        "chunk_id": "c2",
        "text": "本地推理模式的优势：数据不出本机，隐私保护，响应速度快，无需联网。",
        "file_path": "/docs/使用说明.md",
        "page": 3,
    },
    {
        "chunk_id": "c3",
        "text": "云端推理模式的优点：不占用本地算力，适合高性能模型，但需要网络连接。",
        "file_path": "/docs/使用说明.md",
        "page": 5,
    },
    {
        "chunk_id": "c4",
        "text": "RAG 问答支持多轮对话，会根据对话历史理解代词指代。",
        "file_path": "/docs/FAQ.md",
        "page": 1,
    },
]


def _sign(message: str) -> str:
    return hmac.new(_PSK, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _chat(
    client: TestClient,
    seq: int,
    query: str,
    history: list[dict[str, str]],
) -> str:
    """带 HMAC 签名的 POST /chat/stream（fts_chunks 模拟 Rust 层 FTS 命中）。"""
    body = json.dumps(
        {
            "query": query,
            "history": history,
            "table_name": _TABLE,
            "fts_chunks": _CHUNKS,
        },
        ensure_ascii=False,
    )
    resp = client.post(
        _PATH,
        content=body,
        headers={
            "Content-Type": "application/json",
            "X-Signature": _sign(f"POST|{_PATH}|{body}|{seq}"),
            "X-Request-Seq": str(seq),
        },
    )
    assert resp.status_code == 200, resp.text
    return resp.text


def _parse(text: str) -> list[tuple[str, dict[str, object]]]:
    """按 ``event: ...\\ndata: ...`` 帧解析 SSE 文本。"""
    frames: list[tuple[str, dict[str, object]]] = []
    for frame in text.strip().split("\n\n"):
        if not frame:
            continue
        lines = frame.split("\n")
        frames.append(
            (lines[0].split(":", 1)[1].strip(), json.loads(lines[1].split(":", 1)[1].strip()))
        )
    return frames


def _answer(events: list[tuple[str, dict[str, object]]]) -> str:
    """拼接全部 token 事件为完整回答。"""
    return "".join(str(data["content"]) for name, data in events if name == "token")


def _has_error(events: list[tuple[str, dict[str, object]]]) -> bool:
    return any(name == "error" for name, _ in events)


def _sources(events: list[tuple[str, dict[str, object]]]) -> list[dict[str, object]]:
    for name, data in events:
        if name == "search_result":
            return list(data["sources"])  # type: ignore[arg-type]
    return []


def _citations(events: list[tuple[str, dict[str, object]]]) -> list[dict[str, object]]:
    for name, data in events:
        if name == "citation":
            return list(data["citations"])  # type: ignore[arg-type]
    return []


@pytest.mark.e2e
async def test_rag_chat_full_flow() -> None:
    """E2E-003：首轮推理模式问答 + 引用跳转数据 + 第二轮带 history 的代词消解。"""
    if not os.environ.get("RUN_E2E"):
        pytest.skip("需真实 Ollama：设置 RUN_E2E=1 运行")

    # db_path.parent 会被 chmod 0700，需可 chmod 的新建目录（系统 TMPDIR 根目录不行）
    base = Path(tempfile.mkdtemp(prefix="filemind-e2e-"))
    try:
        mgr = LanceDBManager(base / "lancedb")
        mgr.connect()
        mgr.ensure_table("bge-large-zh-v1.5", 1, 1024)
        vecs = await embed_texts([str(c["text"]) for c in _CHUNKS])
        rows = [
            {
                "vector": vec,
                "chunk_id": str(c["chunk_id"]),
                "file_path": str(c["file_path"]),
                "chunk_text": str(c["text"]),
                "page": int(c["page"]),
            }
            for c, vec in zip(_CHUNKS, vecs, strict=False)
        ]
        mgr.open_table(_TABLE).add(rows)

        state.reset_state()
        state.set_psk(_PSK)
        state.set_lancedb(mgr)
        client = TestClient(app)

        # 第 1 轮：流式输出 + 引用跳转数据 + 回答内容
        events1 = _parse(_chat(client, 1, "FileMind 支持哪些推理模式？", history=[]))
        names1 = [name for name, _ in events1]
        assert not _has_error(events1), f"首轮异常: {names1}"
        token_events = [name for name in names1 if name == "token"]
        assert token_events, "无 token 事件（流式输出缺失）"
        answer1 = _answer(events1)
        assert answer1, "首轮回答为空"
        assert "本地推理" in answer1 and "云端推理" in answer1, answer1

        citations1 = _citations(events1)
        assert citations1, "无 citation 事件（引用标注缺失）"
        source_ids1 = {int(s["id"]) for s in _sources(events1)}
        for cite in citations1:
            assert int(cite["id"]) in source_ids1, "citation.id 不在 sources.id 中"
            assert cite["file_name"] and int(cite["page"]) >= 0, "引用跳转数据缺 file/page"
        assert any(name == "done" for name in names1), "缺 done 事件"

        # 第 2 轮：带 history 的追问，验证多轮对话 + 代词消解命中「隐私」
        events2 = _parse(
            _chat(
                client,
                2,
                "本地模式有什么优势？",
                history=[{"user": "FileMind 支持哪些推理模式？", "assistant": answer1}],
            )
        )
        names2 = [name for name, _ in events2]
        assert not _has_error(events2), f"二轮异常: {names2}"
        answer2 = _answer(events2)
        assert answer2, "二轮回答为空"
        assert "隐私" in answer2, f"代词消解未命中隐私文档: {answer2}"
        assert _citations(events2), "二轮缺 citation"
        done2 = [data for name, data in events2 if name == "done"]
        assert done2 and int(done2[0]["total_tokens"]) > 0, "二轮 done 缺 total_tokens"
    finally:
        shutil.rmtree(base, ignore_errors=True)
