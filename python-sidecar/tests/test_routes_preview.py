"""routes_preview 单元测试：/extract/document 文档文本抽取的成功与错误分支。"""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from docx import Document
from fastapi.testclient import TestClient

from app import state as app_state
from app.main import app

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def client() -> TestClient:
    """重置 Sidecar 全局态后返回测试客户端。

    其他用例可能给 `state` 注入了 PSK（HMAC 验签开启）；本路由走 dev 直连，
    先 `reset_state` 回到无 PSK 模式，避免顺序依赖导致的 401。
    """
    app_state.reset_state()
    return TestClient(app)


def _write_docx(path: Path) -> None:
    """用 python-docx 写一个含标记文本的最小 docx。"""
    doc = Document()
    doc.add_paragraph("季度报告正文预览")
    doc.save(str(path))


def test_extract_document_docx_success(client: TestClient, tmp_path: Path) -> None:
    p = tmp_path / "report.docx"
    _write_docx(p)

    resp = client.post("/extract/document", json={"path": str(p)})

    assert resp.status_code == 200
    assert "季度报告正文预览" in resp.json()["text"]


def test_extract_document_missing_path(client: TestClient, tmp_path: Path) -> None:
    resp = client.post("/extract/document", json={"path": str(tmp_path / "ghost.docx")})

    assert resp.status_code == 400
    assert "FILE-E-002" in resp.json()["detail"]


def test_extract_document_unsupported_extension(client: TestClient, tmp_path: Path) -> None:
    p = tmp_path / "note.txt"
    p.write_text("plain text", encoding="utf-8")

    resp = client.post("/extract/document", json={"path": str(p)})

    assert resp.status_code == 400
    assert "FILE-E-005" in resp.json()["detail"]


def test_extract_document_corrupt_raises_422(client: TestClient, tmp_path: Path) -> None:
    p = tmp_path / "broken.docx"
    p.write_bytes(b"not a real zip document")

    resp = client.post("/extract/document", json={"path": str(p)})

    assert resp.status_code == 422
    assert "FILE-E-006" in resp.json()["detail"]
