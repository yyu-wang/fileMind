from fastapi import APIRouter

from app.models import IndexBuildResponse, IndexIncrementalResponse

router = APIRouter(prefix="/index", tags=["索引"])


@router.post("/build", response_model=IndexBuildResponse)
async def build_index() -> IndexBuildResponse:
    """构建文件索引。"""
    return IndexBuildResponse(indexed_count=0, skipped_count=0)


@router.post("/incremental", response_model=IndexIncrementalResponse)
async def incremental_update() -> IndexIncrementalResponse:
    """增量更新索引。"""
    return IndexIncrementalResponse(added=0, updated=0, deleted=0)
