from fastapi import APIRouter

router = APIRouter(prefix="/classify", tags=["分类"])


@router.post("")
async def classify_files() -> dict:
    """对文件列表执行三层分类。"""
    return {"items": [], "stats": {}, "confidence": 0.0}
