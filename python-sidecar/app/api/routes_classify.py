from fastapi import APIRouter

from app.models import ClassifyRequest, ClassifyResponse
from app.services.classify_service import build_default_engine, classify_files

router = APIRouter(prefix="/classify", tags=["分类"])


@router.post("", response_model=ClassifyResponse)
async def classify(request: ClassifyRequest) -> ClassifyResponse:
    """对文件列表执行三层分类：规则 → 启发式 → LLM 兜底。

    规则/启发式命中直接返回；两者均未命中时调用 LLM（P-01），
    低置信度结果标记 ``needs_review`` 待人工确认。
    """
    engine = build_default_engine()
    return await classify_files(request.files, request.categories, engine=engine)
