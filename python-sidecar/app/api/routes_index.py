"""索引管理路由：构建 + 增量。

POST /index/build       — 建立文件索引（T7.x：文本读取 → 分块 → Embedding → 写 LanceDB）
POST /index/incremental — 增量索引（T2.3：双重判断 content_hash + embedding_version）
"""

from __future__ import annotations

import time

from fastapi import APIRouter, HTTPException

from app import state
from app.models import (
    IncrementalChangeRequest,
    IncrementalIndexResponse,
    IndexBuildRequest,
    IndexBuildResponse,
)
from app.services.index_service import evaluate_incremental
from app.services.ingest_service import build_index as build_index_service

router = APIRouter(prefix="/index", tags=["索引"])


@router.post("/build", response_model=IndexBuildResponse)
async def build_index(req: IndexBuildRequest) -> IndexBuildResponse:
    """建立文件索引：读取 → 分块 → Embedding → 写入 LanceDB。

    Args:
        req: 文件列表（file_id + 路径）+ embedding 模型 + 目标表名。

    Returns:
        索引统计（indexed / skipped，按文件计数）。

    Raises:
        HTTPException 503: 向量库未初始化或 Embedding 不可用。
    """
    mgr = state.get_lancedb()
    if mgr is None:
        raise HTTPException(status_code=503, detail="向量库未初始化")
    if not req.table_name:
        raise HTTPException(status_code=400, detail="table_name 不能为空")

    files = [(f.file_id, f.path) for f in req.files]
    try:
        result = await build_index_service(files, req.table_name, mgr, req.embedding_model)
    except Exception as exc:  # noqa: BLE001 - 上层统一转 503（Embedding 不可用等）
        raise HTTPException(status_code=503, detail=f"建立索引失败: {exc}") from exc

    return IndexBuildResponse(
        indexed_count=result.indexed,
        skipped_count=result.skipped,
    )


@router.post("/incremental", response_model=IncrementalIndexResponse)
async def incremental_update(req: IncrementalChangeRequest) -> IncrementalIndexResponse:
    """增量索引：content_hash + embedding_version 双重判断。

    流程（API 规格书 POST /api/v1/index/incremental）：
        1. 对每个 change 执行双重判断
        2. 返回 indexed / skipped / deleted 计数 + 耗时

    注意：本路由只做"判断"，不执行实际的 LanceDB 向量增删——
    后续 T2.5 Repository 层会根据 indexed 列表做真实 embedding 写入。
    """
    t0 = time.time()
    result = evaluate_incremental(req.changes, req.embedding_version)
    duration_ms = int((time.time() - t0) * 1000)

    return IncrementalIndexResponse(
        indexed=len(result.indexed),
        skipped=len(result.skipped),
        deleted=len(result.deleted),
        duration_ms=duration_ms,
    )
