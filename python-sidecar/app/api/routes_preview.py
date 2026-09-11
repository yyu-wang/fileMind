"""二进制文档文本抽取路由（供 Rust 文件预览代理调用）。

复用 RAG 索引同款 ``doc_extract`` 抽取器：PDF / DOCX / XLSX / PPTX → 纯文本，
供前端"文档预览抽屉"展示。仅本机 Rust 代理可触达（HMAC 中间件保护）；
路径已由 Rust ``security::validate`` 校验，这里对存在性/扩展名做兜底防御。
"""

from __future__ import annotations

import asyncio
from pathlib import Path

from fastapi import APIRouter, HTTPException

from app.models.doc_preview import DocumentExtractRequest, DocumentExtractResponse
from app.services.doc_extract import (
    DocumentExtractError,
    extract_document_text,
    is_binary_document,
)

router = APIRouter(prefix="/extract", tags=["文档抽取"])


@router.post("/document", response_model=DocumentExtractResponse)
async def extract_document(request: DocumentExtractRequest) -> DocumentExtractResponse:
    """抽取二进制文档正文为纯文本（供文件预览）。

    Args:
        request: 含待抽取文件绝对路径的请求体。

    Returns:
        抽取出的纯文本内容。

    Raises:
        HTTPException 400: 路径不存在 / 不是文件 / 扩展名不受支持
        HTTPException 422: 文档解析失败（损坏 / 加密 / 权限）
    """
    path = Path(request.path)
    if not path.is_file():
        raise HTTPException(status_code=400, detail="FILE-E-002:预览对象不存在或不是文件")
    if not is_binary_document(path):
        raise HTTPException(status_code=400, detail="FILE-E-005:暂不支持该文档类型")
    try:
        text = await asyncio.to_thread(extract_document_text, path)
    except DocumentExtractError as exc:
        raise HTTPException(status_code=422, detail=f"FILE-E-006:文档解析失败（{exc}）") from exc
    return DocumentExtractResponse(text=text)
