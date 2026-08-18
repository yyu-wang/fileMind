"""索引管理路由：构建 + 增量。

POST /index/build       — 首次索引 / 分批重建（T2.4+ 接入 LanceDB）
POST /index/incremental — 增量索引（T2.3：双重判断 content_hash + embedding_version）
"""

from __future__ import annotations

import time

from fastapi import APIRouter

from app.models import (
    IncrementalChangeRequest,
    IncrementalIndexResponse,
    IndexBuildResponse,
)
from app.services.index_service import evaluate_incremental

router = APIRouter(prefix="/index", tags=["索引"])


@router.post("/build", response_model=IndexBuildResponse)
async def build_index() -> IndexBuildResponse:
    """构建文件索引（桩：T2.4+ 接入 LanceDB 向量写入后补全）。"""
    return IndexBuildResponse(indexed_count=0, skipped_count=0)


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
