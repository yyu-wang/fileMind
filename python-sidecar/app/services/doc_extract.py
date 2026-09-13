"""二进制文档文本抽取服务（供 RAG 向量索引使用）。

支持 PDF / DOCX / XLSX / PPTX 四种办公格式：把二进制内容抽取为纯文本，
交回 ``ingest_service`` 走统一的分块 + Embedding 链路。解析库在函数体内
**惰性导入**——不影响 Sidecar 冷启动与模块加载速度（PyInstaller 仍能通过
源码里的 import 语句静态收集依赖）。

设计约束：
- 字符上限 :data:`MAX_EXTRACT_CHARS`：超限截断并告警，防超大文档耗尽
  embedding 预算（对齐文本类 ``MAX_FILE_BYTES`` 截断的同一意图）。
- 解析异常（损坏/加密/权限/未知类型）统一包装为 :class:`DocumentExtractError`，
  由调用方按「单文件跳过」处理，不中断整批索引。

边界说明：本模块只做「二进制 → 文本」。Rust 侧 SQLite FTS5（file_fts.content）
因无 Python 解析依赖仍只覆盖纯文本文件——二进制文档通过 LanceDB 向量腿参与
问答，关键词腿命中的缺失由向量语义召回兜底。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.core.logging import getLogger

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

logger = getLogger("filemind.doc_extract")

#: 抽取文本字符上限（防超大文档耗尽 embedding 预算；放宽到 5M 字符以覆盖 50MB 级大文档正文）
MAX_EXTRACT_CHARS: int = 5_000_000


class DocumentExtractError(Exception):
    """文档抽取失败（损坏 / 加密 / 权限 / 不支持的格式）。

    Attributes:
        message: 面向日志的中文简述（不含完整文件路径，防敏感泄漏）。
    """


def _extension(path: Path) -> str:
    """返回小写扩展名（无扩展名返回空串）。"""
    return path.suffix.lstrip(".").lower() if path.suffix else ""


def _extract_pdf(path: Path) -> list[str]:
    """抽取 PDF 各页文本（pypdf 惰性导入）。

    Args:
        path: PDF 文件路径。

    Raises:
        DocumentExtractError: PDF 已加密。

    Returns:
        按页拆分出的非空文本行。
    """
    from pypdf import PdfReader

    reader = PdfReader(str(path))
    if reader.is_encrypted:
        raise DocumentExtractError("PDF 已加密，无法抽取文本")
    lines: list[str] = []
    for page in reader.pages:
        try:
            raw = page.extract_text() or ""
        except Exception as exc:  # noqa: BLE001 - 单页失败不中断整份文档
            logger.warning("doc_extract.pdf_page_failed", error=type(exc).__name__)
            continue
        for raw_line in raw.splitlines():
            cleaned = raw_line.strip()
            if cleaned:
                lines.append(cleaned)
    return lines


def _extract_docx(path: Path) -> list[str]:
    """抽取 Word 正文段落与表格单元格文本。

    Args:
        path: .docx 文件路径。

    Returns:
        非空文本行（表格行用 `` | `` 连接单元格）。
    """
    from docx import Document

    doc = Document(str(path))
    lines: list[str] = []
    for para in doc.paragraphs:
        cleaned = para.text.strip()
        if cleaned:
            lines.append(cleaned)
    for table in doc.tables:
        for row in table.rows:
            cells = [cell.text.strip() for cell in row.cells]
            non_empty = [cell for cell in cells if cell]
            if non_empty:
                lines.append(" | ".join(non_empty))
    return lines


def _extract_xlsx(path: Path) -> list[str]:
    """抽取 Excel 各工作表单元格文本。

    Args:
        path: .xlsx 文件路径。

    Returns:
        非空文本行（每行单元格用 `` | `` 连接；值统一转字符串）。
    """
    from openpyxl import load_workbook

    wb = load_workbook(str(path), read_only=True, data_only=True)
    lines: list[str] = []
    try:
        for sheet in wb.worksheets:
            for row in sheet.iter_rows(values_only=True):
                values = [str(value) for value in row if value is not None]
                if values:
                    lines.append(" | ".join(values))
    finally:
        wb.close()
    return lines


def _extract_pptx(path: Path) -> list[str]:
    """抽取 PPT 各幻灯片文本框架与表格文本。

    Args:
        path: .pptx 文件路径。

    Returns:
        非空文本行（表格行用 `` | `` 连接单元格）。
    """
    from pptx import Presentation

    prs = Presentation(str(path))
    lines: list[str] = []
    for slide in prs.slides:
        for shape in slide.shapes:
            if shape.has_text_frame:
                cleaned = shape.text_frame.text.strip()
                if cleaned:
                    lines.append(cleaned)
            if shape.has_table:
                for row in shape.table.rows:
                    cells = [cell.text.strip() for cell in row.cells]
                    non_empty = [cell for cell in cells if cell]
                    if non_empty:
                        lines.append(" | ".join(non_empty))
    return lines


#: 二进制文档扩展名 → 抽取器映射（小写、无点前缀）
_EXTRACTORS: dict[str, Callable[[Path], list[str]]] = {
    "pdf": _extract_pdf,
    "docx": _extract_docx,
    "xlsx": _extract_xlsx,
    "pptx": _extract_pptx,
}

#: 支持抽取的二进制文档扩展名集合
DOC_EXTENSIONS: frozenset[str] = frozenset(_EXTRACTORS)


def is_binary_document(path: Path) -> bool:
    """文件扩展名是否属于可抽取的二进制文档。

    Args:
        path: 文件绝对路径。

    Returns:
        ``True`` 表示扩展名命中 PDF/DOCX/XLSX/PPTX 之一。
    """
    return _extension(path) in DOC_EXTENSIONS


def _join_capped(lines: list[str]) -> str:
    """把文本行拼为单字符串，超限截断（最多记一次告警）。

    Args:
        lines: 抽取出的文本行（已去首尾空白）。

    Returns:
        拼接结果；超 :data:`MAX_EXTRACT_CHARS` 时截断到上限。
    """
    collected: list[str] = []
    total = 0
    truncated = False
    for line in lines:
        remain = MAX_EXTRACT_CHARS - total
        if remain <= 0:
            truncated = True
            break
        if len(line) > remain:
            collected.append(line[:remain])
            total = MAX_EXTRACT_CHARS
            truncated = True
            break
        collected.append(line)
        total += len(line)
    if truncated:
        logger.warning("doc_extract.truncated", limit=MAX_EXTRACT_CHARS)
    return "\n".join(collected)


def extract_document_text(path: Path) -> str:
    """抽取二进制文档正文为纯文本（按扩展名分发）。

    Args:
        path: PDF / DOCX / XLSX / PPTX 文件绝对路径。

    Returns:
        抽取正文；空文档返回空串。

    Raises:
        DocumentExtractError: 扩展名不受支持、文件损坏 / 加密 / 不可读。
    """
    ext = _extension(path)
    extractor = _EXTRACTORS.get(ext)
    if extractor is None:
        raise DocumentExtractError(f"不支持的文档扩展名: .{ext}")
    try:
        lines = extractor(path)
    except DocumentExtractError:
        raise
    except Exception as exc:
        raise DocumentExtractError(f"{ext.upper()} 解析失败({type(exc).__name__})") from exc
    return _join_capped(lines)
