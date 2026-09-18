"""索引链路的文本层：扩展名判定、可索引正文读取、分块（原 ``ingest_service.py`` 拆出）。

链路（打通问答的检索数据源）：
    文件路径 → 读取文本（仅文本类，PDF/Word 依赖未装暂缓）→ 分块（~500 字符）
    → Embedding 批量向量化 → 写入 LanceDB documents_{model}_v{version}。

本模块只负责「文件 → 文本块」，不碰 Embedding 与 LanceDB；编排在
:mod:`app.services.ingest_service`。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.services.doc_extract import extract_document_text, is_binary_document

if TYPE_CHECKING:
    from pathlib import Path

#: 支持索引的文本扩展名（与 file_preview.rs 文本类对齐）
TEXT_EXTENSIONS: set[str] = {
    "txt",
    "md",
    "log",
    "json",
    "yaml",
    "yml",
    "csv",
    "xml",
    "toml",
    "ini",
    "conf",
    "ts",
    "tsx",
    "js",
    "jsx",
    "py",
    "rs",
    "go",
    "java",
    "c",
    "h",
    "cpp",
    "css",
    "html",
    "sh",
    "sql",
}

#: 单文件读取上限（50MB）。超限截断，避免超大文本耗尽内存/embedding 预算
MAX_FILE_BYTES: int = 50 * 1024 * 1024
#: 分块目标长度（字符），对齐 P-03 上下文片段 CONTENT_MAX=500
CHUNK_TARGET_CHARS: int = 500


def _extension(path: Path) -> str:
    """返回小写扩展名（无扩展名返回空串）。"""
    return path.suffix.lstrip(".").lower() if path.suffix else ""


def is_supported_text(path: Path) -> bool:
    """文件扩展名是否属于可索引文本类。"""
    return _extension(path) in TEXT_EXTENSIONS


def read_indexable_text(path: Path) -> str | None:
    """按扩展名读取可索引正文：纯文本直读或二进制文档抽取。

    - 文本类（TEXT_EXTENSIONS）：直接 UTF-8 读取（见 :func:`read_text`）；
    - 二进制文档（pdf/docx/xlsx/pptx）：经 :func:`extract_document_text` 抽取；
    - 其余类型返回 ``None``（调用方跳过）。

    Args:
        path: 文件绝对路径。

    Returns:
        可索引正文；非索引类型返回 ``None``。

    Raises:
        OSError: 文本文件不可读（非 UTF-8 字节按替换符降级）。
        DocumentExtractError: 二进制文档解析失败。
    """
    if _extension(path) in TEXT_EXTENSIONS:
        return read_text(path)
    if is_binary_document(path):
        return extract_document_text(path)
    return None


def read_text(path: Path) -> str:
    """读取文本文件内容（截断到 :data:`MAX_FILE_BYTES`）。

    非 UTF-8 字节用 ``errors="replace"`` 降级，避免单文件编码异常中断整体。

    Args:
        path: 文件绝对路径。

    Raises:
        OSError: 文件不可读（调用方计入 skipped）。
    """
    with path.open("rb") as fh:
        data = fh.read(MAX_FILE_BYTES)
    return data.decode("utf-8", errors="replace")


def chunk_text(text: str, target: int = CHUNK_TARGET_CHARS) -> list[str]:
    """把文本按行累积切分为 ~target 字符的块。

    空行作为段落边界：空行处必切（当前块非空时），保证段落不被拆散；
    无空行时按行累积到超 target 才切。行长度含换行符（``len(line) + 1``），
    使块内容长度（含换行）贴近 target。

    Args:
        text: 原始文本。
        target: 单块目标字符数。

    Returns:
        分块列表；空文本返回空列表。
    """
    if not text.strip():
        return []
    blocks: list[str] = []
    current: list[str] = []
    current_len = 0
    for line in text.splitlines():
        # 空行 = 段落边界：结束当前块（空行本身不进入块内容）
        if line.strip() == "":
            if current:
                blocks.append("\n".join(current))
                current = []
                current_len = 0
            continue
        line_cost = len(line) + 1  # 含行尾换行
        if current and current_len + line_cost > target:
            blocks.append("\n".join(current))
            current = []
            current_len = 0
        current.append(line)
        current_len += line_cost
    if current:
        blocks.append("\n".join(current))
    return blocks
