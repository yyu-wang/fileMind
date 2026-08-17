from fastapi import APIRouter

from app.models import EmbeddingModelsResponse, EmbeddingSwitchResponse

router = APIRouter(prefix="/embedding", tags=["Embedding"])


@router.get("/models", response_model=EmbeddingModelsResponse)
async def list_models() -> EmbeddingModelsResponse:
    """列出可用的 Embedding 模型。"""
    return EmbeddingModelsResponse(models=["bge-large-zh-v1.5", "bge-m3"])


@router.post("/switch", response_model=EmbeddingSwitchResponse)
async def switch_model() -> EmbeddingSwitchResponse:
    """切换 Embedding 模型。"""
    return EmbeddingSwitchResponse(status="ok", model="")
