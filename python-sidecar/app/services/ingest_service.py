"""T7.x — 文件索引服务：文本读取 + 分块 + Embedding + 写入 LanceDB。

链路（打通问答的检索数据源）：
    文件路径 → 读取文本（仅文本类，PDF/Word 依赖未装暂缓）→ 分块（~500 字符）
    → Ollama Embedding 批量向量化 → 写入 LanceDB documents_{model}_v{version}。

错误策略：
    - 单文件读取/非文本/空内容 → 计入 skipped，不中断整体
    - Ollama Embedding 不可用 → 抛 :class:`EmbeddingUnavailableError`（整体失败）

模块划分（原单文件 378 行，逼近 Python 模块 500 行强制阈值，见 `rules/complexity.md`）：
    - :mod:`app.services.ingest_text`    扩展名判定 / 正文读取 / 分块（「文件 → 文本块」）
    - :mod:`app.services.ingest_vectors` 已存在向量行的删除与路径同步
    - 本模块：Embedding 分批与建索引编排（``build_index`` 是唯一入口）
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path

from app.core.logging import getLogger, sanitize_path
from app.db.lancedb_repo import DocumentChunk, LanceDBManager
from app.services.doc_extract import DocumentExtractError
from app.services.embedding_service import EMBEDDING_MODEL, embed_texts
from app.services.ingest_text import chunk_text, read_indexable_text
from app.services.ingest_vectors import _delete_stale_chunks

logger = getLogger("filemind.ingest")

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


async def _embed_docs(docs: list[DocumentChunk], embedding_model: str) -> None:
    """分批 Embedding，向量就地写回 ``doc.vector``（网络 IO，事件循环友好）。"""
    for start in range(0, len(docs), EMBED_BATCH_SIZE):
        batch = docs[start : start + EMBED_BATCH_SIZE]
        vectors = await embed_texts(
            [d.chunk_text for d in batch],
            model=embedding_model,
        )
        for doc, vector in zip(batch, vectors, strict=True):
            doc.vector = vector


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
    # 阶段 2：分批 Embedding
    await _embed_docs(docs, embedding_model)
    # 阶段 3（SC-C3）：先删旧行再写入——失败可整体重跑，无重复行
    _delete_stale_chunks(mgr, table_name, indexed_file_ids)
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
