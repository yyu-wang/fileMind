from fastapi import APIRouter

router = APIRouter(prefix="/chat", tags=["RAG 问答"])


@router.post("/query")
async def query() -> dict:
    """RAG 问答查询。"""
    return {"answer": "", "citations": [], "tokens": 0}


@router.post("/query/stream")
async def query_stream() -> dict:
    """RAG 问答流式输出。"""
    return {"status": "streaming"}
