from fastapi import APIRouter

router = APIRouter(prefix="/embedding", tags=["Embedding"])


@router.get("/models")
async def list_models() -> dict:
    """列出可用的 Embedding 模型。"""
    return {"models": ["bge-large-zh-v1.5", "bge-m3"]}


@router.post("/switch")
async def switch_model() -> dict:
    """切换 Embedding 模型。"""
    return {"status": "ok", "model": ""}
