"""ingest_service 单元测试：分块、文本读取、支持判定、build_index 流程（mock Embedding）。"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest
from docx import Document

from app.services import ingest_service

if TYPE_CHECKING:
    from app.db.lancedb_repo import DocumentChunk


def test_is_supported_text() -> None:
    assert ingest_service.is_supported_text(Path("/tmp/a.md"))
    assert ingest_service.is_supported_text(Path("/tmp/a.py"))
    assert ingest_service.is_supported_text(Path("/tmp/data.csv"))
    assert not ingest_service.is_supported_text(Path("/tmp/a.pdf"))
    assert not ingest_service.is_supported_text(Path("/tmp/a.png"))
    assert not ingest_service.is_supported_text(Path("/tmp/noext"))


def test_chunk_text_single_short() -> None:
    chunks = ingest_service.chunk_text("hello world\nsecond line")
    assert len(chunks) == 1
    assert "hello world" in chunks[0]


def test_chunk_text_splits_long() -> None:
    # 单行超 target → 按行累积切块；200 个 'a' 行，每行 1 字符，target=500 → 应切成多块
    text = "\n".join("a" for _ in range(2000))
    chunks = ingest_service.chunk_text(text)
    assert len(chunks) > 1
    assert all(len(c) <= ingest_service.CHUNK_TARGET_CHARS + 1 for c in chunks)


def test_chunk_text_paragraph_boundary() -> None:
    # 空行分段：段内短文本不切，空行处收块
    text = "line1\nline2\n\nline3\nline4"
    chunks = ingest_service.chunk_text(text)
    assert len(chunks) == 2
    assert chunks[0] == "line1\nline2"
    assert chunks[1] == "line3\nline4"


def test_chunk_text_empty() -> None:
    assert ingest_service.chunk_text("") == []
    assert ingest_service.chunk_text("   \n  ") == []


def test_read_text(tmp_path: Path) -> None:
    p = tmp_path / "a.txt"
    p.write_text("hello 世界", encoding="utf-8")
    assert ingest_service.read_text(p) == "hello 世界"


def test_read_text_truncates_oversize(tmp_path: Path) -> None:
    p = tmp_path / "big.txt"
    p.write_bytes(b"a" * (ingest_service.MAX_FILE_BYTES + 100))
    assert len(ingest_service.read_text(p)) == ingest_service.MAX_FILE_BYTES


def test_read_text_invalid_utf8_lossy(tmp_path: Path) -> None:
    p = tmp_path / "bin.txt"
    p.write_bytes(b"\xff\xfe\x00abc")
    # 非法字节降级为替换符，不抛异常
    assert "abc" in ingest_service.read_text(p)


class _FakeManager:
    """LanceDBManager 最小替身：记录 add_chunks / delete 调用。"""

    def __init__(self) -> None:
        self.added: list[DocumentChunk] = []
        self.deleted: list[str] = []

    def add_chunks(self, table_name: str, chunks: list[DocumentChunk]) -> int:
        assert table_name == "documents_bge-large-zh-v1.5_v1"
        self.added.extend(chunks)
        return len(chunks)

    def delete_chunks_by_file_id(self, table_name: str, file_id: str) -> int:
        # SC-C3：build_index 写入前按 file_id 清旧行（fake 只记录调用）
        self.deleted.append(file_id)
        return 0


@pytest.mark.asyncio
async def test_build_index_writes_text_files(tmp_path: Path) -> None:
    """文本文件 → 分块 → embedding → 写入；非文本跳过。"""
    txt = tmp_path / "note.md"
    txt.write_text("第一行\n第二行\n\n第三行", encoding="utf-8")
    png = tmp_path / "img.png"
    png.write_bytes(b"png")

    mgr = _FakeManager()
    files = [
        ("f1", str(txt)),
        ("f2", str(png)),
    ]

    async def fake_embed(texts: list[str], model: str) -> list[list[float]]:
        assert model == "bge-large-zh-v1.5"
        return [[1.0, 2.0] for _ in texts]

    with patch.object(ingest_service, "embed_texts", side_effect=fake_embed):
        result = await ingest_service.build_index(files, "documents_bge-large-zh-v1.5_v1", mgr)

    assert result.indexed == 1
    assert result.skipped == 1  # png 跳过
    assert len(mgr.added) == 2  # note.md 分 2 块（空行分段）
    assert all(d.vector == [1.0, 2.0] for d in mgr.added)
    assert all(d.file_path == str(txt) for d in mgr.added)
    # chunk_id 规范：{file_id}-{seq}
    assert {d.chunk_id for d in mgr.added} == {"f1-0", "f1-1"}


@pytest.mark.asyncio
async def test_build_index_empty_result_no_write(tmp_path: Path) -> None:
    """全部跳过（非文本）→ 不写库、indexed=0。"""
    png = tmp_path / "img.png"
    png.write_bytes(b"png")

    mgr = _FakeManager()
    result = await ingest_service.build_index(
        [("f1", str(png))],
        "documents_bge-large-zh-v1.5_v1",
        mgr,
    )
    assert result.indexed == 0
    assert result.skipped == 1
    assert mgr.added == []


@pytest.mark.asyncio
async def test_build_index_extracts_binary_docx(tmp_path: Path) -> None:
    """docx 二进制文档 → 抽取 → 分块 → 写入向量索引。"""
    doc_path = tmp_path / "report.docx"
    doc = Document()
    doc.add_paragraph("二进制文档正文甲")
    doc.add_paragraph("正文乙段落")
    doc.save(str(doc_path))

    mgr = _FakeManager()

    async def fake_embed(texts: list[str], model: str) -> list[list[float]]:
        assert model == "bge-large-zh-v1.5"
        return [[1.0, 2.0] for _ in texts]

    with patch.object(ingest_service, "embed_texts", side_effect=fake_embed):
        result = await ingest_service.build_index(
            [("f1", str(doc_path))],
            "documents_bge-large-zh-v1.5_v1",
            mgr,
        )

    assert result.indexed == 1
    assert result.skipped == 0
    joined = " ".join(c.chunk_text for c in mgr.added)
    assert "二进制文档正文甲" in joined
    assert "正文乙段落" in joined
    assert {d.chunk_id for d in mgr.added} == {"f1-0"}


@pytest.mark.asyncio
async def test_build_index_skips_corrupt_binary_but_indexes_others(
    tmp_path: Path,
) -> None:
    """损坏的二进制文档计入 skipped，不中断同批正常文件。"""
    broken = tmp_path / "broken.pdf"
    broken.write_bytes(b"not a real pdf")
    md = tmp_path / "ok.md"
    md.write_text("正常正文", encoding="utf-8")

    mgr = _FakeManager()

    async def fake_embed(texts: list[str], model: str) -> list[list[float]]:
        return [[1.0, 2.0] for _ in texts]

    with patch.object(ingest_service, "embed_texts", side_effect=fake_embed):
        result = await ingest_service.build_index(
            [("f1", str(broken)), ("f2", str(md))],
            "documents_bge-large-zh-v1.5_v1",
            mgr,
        )

    assert result.indexed == 1
    assert result.skipped == 1
    joined = " ".join(c.chunk_text for c in mgr.added)
    assert "正常正文" in joined
