from fastapi import APIRouter

from app.models import ChatQueryResponse, ChatStreamResponse

router = APIRouter(prefix="/chat", tags=["RAG 问答"])


@router.post("/query", response_model=ChatQueryResponse)
async def query() -> ChatQueryResponse:
    """RAG 问答查询。"""
    return ChatQueryResponse(answer="", citations=[], tokens=0)


@router.post("/query/stream", response_model=ChatStreamResponse)
async def query_stream() -> ChatStreamResponse:
    """RAG 问答流式输出。"""
    return ChatStreamResponse(status="streaming")
