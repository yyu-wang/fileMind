"""T7.x — 文件索引服务：文本读取 + 分块 + Embedding + 写入 LanceDB。

链路（打通问答的检索数据源）：
    文件路径 → 读取文本（仅文本类，PDF/Word 依赖未装暂缓）→ 分块（~500 字符）
    → Ollama Embedding 批量向量化 → 写入 LanceDB documents_{model}_v{version}。

错误策略：
    - 单文件读取/非文本/空内容 → 计入 skipped，不中断整体
    - Ollama Embedding 不可用 → 抛 :class:`EmbeddingUnavailableError`（整体失败）
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path

from app.core.logging import getLogger, sanitize_path
from app.db.lancedb_repo import DocumentChunk, LanceDBManager
from app.services.doc_extract import (
    DocumentExtractError,
    extract_document_text,
    is_binary_document,
)
from app.services.embedding_service import EMBEDDING_MODEL, embed_texts

logger = getLogger("filemind.ingest")

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

#: 单文件读取上限（2MB）。超限截断，避免超大文本耗尽内存/embedding 预算
MAX_FILE_BYTES: int = 2 * 1024 * 1024
#: 分块目标长度（字符），对齐 P-03 上下文片段 CONTENT_MAX=500
CHUNK_TARGET_CHARS: int = 500
#: 单次 Embedding 调用最大文本条数（对齐并发/内存预算）
EMBED_BATCH_SIZE: int = 20


@dataclass(frozen=True)
class BuildIndexResult:
    """一次 /index/build 的结果统计。"""

    indexed: int
    """成功索引的文件数（写入 ≥1 个 chunk）。"""

    skipped: int
    """跳过的文件数（非文本 / 读取失败 / 空内容）。"""

    indexed_file_ids: tuple[str, ...] = ()
    """实际写入向量的 file_id（供 Rust 回写索引状态标记）。"""


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


def _read_and_chunk(files: list[tuple[str, str]]) -> list[DocumentChunk]:
    """同步读取 + 分块（SC-C1：整体放线程池执行，不卡事件循环）。

    单文件失败（不存在/非文本/不可读/空内容）计入 warning 日志并跳过，
    不中断整体；返回值为空表示本次无可写入内容。
    """
    docs: list[DocumentChunk] = []
    for file_id, raw_path in files:
        path = Path(raw_path)
        if not path.is_file():
            continue
        try:
            text = read_indexable_text(path)
        except (OSError, DocumentExtractError) as exc:
            logger.warning(
                "ingest.read_failed",
                file_id=file_id,
                path=sanitize_path(raw_path),
                error=str(exc),
            )
            continue
        if text is None or not text.strip():
            continue
        chunks = chunk_text(text)
        if not chunks:
            continue
        for seq, content in enumerate(chunks):
            docs.append(
                DocumentChunk(
                    vector=[],
                    chunk_id=f"{file_id}-{seq}",
                    file_path=raw_path,
                    chunk_text=content,
                    page=0,
                )
            )
    return docs


async def build_index(
    files: list[tuple[str, str]],
    table_name: str,
    mgr: LanceDBManager,
    embedding_model: str = EMBEDDING_MODEL,
) -> BuildIndexResult:
    """对文件列表执行索引：读取 → 分块 → Embedding → 写 LanceDB。

    SC-C1：文件读取/分块（I/O + CPU 密集）与 LanceDB 写盘全部经
    ``asyncio.to_thread`` 下沉线程池，事件循环保持可调度——索引期间
    ``/health`` 仍毫秒级响应，避免 Rust watchdog 误判 sidecar 死亡。

    SC-C3：写入前按 file_id 删除旧向量行（add_chunks 是纯追加语义，
    不清理则同文件重复 build 后 chunk 翻倍、检索重复）。仅对本次产出
    ≥1 chunk 的文件执行删除——读取失败/非文本的文件保留旧向量
    （临时权限问题不应连带删掉既有好数据）。

    Args:
        files: ``(file_id, path)`` 列表（来自 Rust SQLite files 表）。
        table_name: 目标向量表名（``documents_{model}_v{version}``）。
        mgr: LanceDB 管理器（表已 ensure）。
        embedding_model: Embedding 模型名。

    Returns:
        索引统计（indexed / skipped，按文件计数）。

    Raises:
        EmbeddingUnavailableError: Ollama Embedding 不可用（整体失败）。
    """
    # 阶段 1：读取 + 分块（同步块整体下沉线程池；skipped 按产出文件数推算）
    docs = await asyncio.to_thread(_read_and_chunk, files)
    indexed_file_ids = {d.chunk_id.rsplit("-", 1)[0] for d in docs}
    indexed = len(indexed_file_ids)
    skipped = len(files) - indexed

    if not docs:
        return BuildIndexResult(indexed=0, skipped=skipped)
    # 阶段 2：分批 Embedding（网络 IO，事件循环友好）
    for start in range(0, len(docs), EMBED_BATCH_SIZE):
        batch = docs[start : start + EMBED_BATCH_SIZE]
        vectors = await embed_texts(
            [d.chunk_text for d in batch],
            model=embedding_model,
        )
        for doc, vector in zip(batch, vectors, strict=True):
            doc.vector = vector

    # 阶段 3（SC-C3）：先删旧行再写入——失败可整体重跑，无重复行
    for file_id in indexed_file_ids:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.delete_stale_skipped", file_id=file_id)
            continue
        mgr.delete_chunks_by_file_id(table_name, file_id)

    # 阶段 4：写入（LanceDB add 同步写盘，下沉线程池）
    await asyncio.to_thread(mgr.add_chunks, table_name, docs)
    logger.info(
        "ingest.done",
        indexed=indexed,
        skipped=skipped,
        chunks=len(docs),
        table=table_name,
    )
    return BuildIndexResult(
        indexed=indexed,
        skipped=skipped,
        indexed_file_ids=tuple(sorted(indexed_file_ids)),
    )


def _is_safe_file_id(value: str) -> bool:
    """file_id 白名单校验：仅允许字母/数字/连字符。

    用于拼入 ``chunk_id LIKE '{file_id}-%'`` 谓词前的防御性检查——
    挡掉引号、空格、``%``/``_`` 通配符、``;`` 等注入/误匹配字符；
    真实 file_id（uuid 十六进制+连字符）必过。命中异常值时跳过该条。
    """
    return bool(value) and all(ch.isalnum() or ch == "-" for ch in value)


def update_paths(
    table_name: str,
    mappings: list[tuple[str, str]],
    mgr: LanceDBManager,
) -> int:
    """把指定文件的最新路径同步到向量索引（chunk_id 前缀匹配，不重新 embedding）。

    分类移动/撤销后调用：SQLite 的 ``files.path`` 已是新路径，但向量行里的
    ``file_path`` 仍是旧路径（增量索引只认 created/modified/deleted，无移动语义）。
    这里按 ``chunk_id = {file_id}-{seq}`` 前缀原地更新 ``file_path``，向量不变。
    表不存在（从未建索引）或 ``mappings`` 为空时返回 0（静默跳过）。

    Args:
        table_name: 目标向量表名（``documents_{model}_v{version}``）。
        mappings: ``(file_id, 最新路径)`` 列表。
        mgr: LanceDB 管理器。

    Returns:
        成功更新路径的文件数（一个文件可对应多个分块行）。
    """
    if not mappings:
        return 0
    if not mgr.is_table_exists(table_name):
        return 0

    tbl = mgr.open_table(table_name)
    updated = 0
    for file_id, path in mappings:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.update_paths_skipped", file_id=file_id)
            continue
        # SC-m16：LIKE 前缀加边界——chunk_id 格式是 {file_id}-{seq}，
        # file_id 是 UUID（含连字符），{file_id}- 已含分隔符，
        # 但显式 ESCAPE 防御 file_id 是另一个 id 前缀的极端场景
        tbl.update(
            where=f"chunk_id = '{file_id}' OR chunk_id LIKE '{file_id}-%' ESCAPE '\\'",
            values={"file_path": path},
        )
        updated += 1
    # SC-m16：updated 计数是有效映射数（LanceDB update 不返回影响行数）
    return updated


def delete_by_file_ids(
    table_name: str,
    file_ids: list[str],
    mgr: LanceDBManager,
) -> int:
    """从向量索引中删除指定文件的全部向量行（目录级移除用）。

    逐个调用 :meth:`LanceDBManager.delete_chunks_by_file_id`，表不存在或
    ``file_ids`` 为空时返回 0（静默跳过）。非法 file_id 跳过并告警，
    不中断整体。

    Args:
        table_name: 目标向量表名（``documents_{model}_v{version}``）。
        file_ids: 待删除向量的文件 ID 列表。
        mgr: LanceDB 管理器。

    Returns:
        成功删除向量的文件数（一个文件对应多个分块行，计数按文件计）。
    """
    if not file_ids:
        return 0
    if not mgr.is_table_exists(table_name):
        return 0

    deleted = 0
    for file_id in file_ids:
        if not _is_safe_file_id(file_id):
            logger.warning("ingest.delete_by_file_ids_skipped", file_id=file_id)
            continue
        mgr.delete_chunks_by_file_id(table_name, file_id)
        deleted += 1
    return deleted
