from fastapi import APIRouter

from app.models import ClassifyResponse

router = APIRouter(prefix="/classify", tags=["分类"])


@router.post("", response_model=ClassifyResponse)
async def classify_files() -> ClassifyResponse:
    """对文件列表执行三层分类。"""
    return ClassifyResponse(items=[], stats={}, confidence=0.0)
