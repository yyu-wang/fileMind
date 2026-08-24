from fastapi import APIRouter

from app.models import ClassifyRequest, ClassifyResponse
from app.services.classify_service import build_default_engine, classify_files
from app.services.provider_factory import resolve_cloud_provider

router = APIRouter(prefix="/classify", tags=["分类"])


@router.post("", response_model=ClassifyResponse)
async def classify(request: ClassifyRequest) -> ClassifyResponse:
    """对文件列表执行三层分类：规则 → 启发式 → LLM 兜底。

    规则/启发式命中直接返回；两者均未命中时调用 LLM（P-01），
    低置信度结果标记 ``needs_review`` 待人工确认。

    ``request.llm_model`` 指定云端模型时（gpt-* / deepseek-*）走云端推理；
    空串或本地模型回落默认本地 Ollama 路径。
    """
    engine = build_default_engine()
    provider = resolve_cloud_provider(request.llm_model)
    return await classify_files(request.files, request.categories, engine=engine, provider=provider)
