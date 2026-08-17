from fastapi import APIRouter

router = APIRouter(prefix="/index", tags=["索引"])


@router.post("/build")
async def build_index() -> dict:
    """构建文件索引。"""
    return {"indexed_count": 0, "skipped_count": 0}


@router.post("/incremental")
async def incremental_update() -> dict:
    """增量更新索引。"""
    return {"added": 0, "updated": 0, "deleted": 0}
