"""搜索路由：构造 FTS5 查询表达式。

POST /search — 接收原始查询词，用 jieba 分词后构造 FTS5 MATCH 表达式返回。

设计约束（项目硬约束）：
    - Sidecar 不直接访问 SQLite，所有 DB 操作由 Rust 层执行
    - 本路由仅做"查询构造"——jieba 分词 + 转义拼装，不执行任何 SQL
    - 调用方（Rust/Tauri 层）拿到 ``fts_query`` 后通过 rusqlite 执行 MATCH
"""

from __future__ import annotations

from fastapi import APIRouter

from app.models import SearchRequest, SearchResponse
from app.services.search_service import build_fts_query, tokenize_chinese

router = APIRouter(prefix="/search", tags=["搜索"])


@router.post("", response_model=SearchResponse)
async def search(req: SearchRequest) -> SearchResponse:
    """构造 FTS5 MATCH 查询表达式。

    流程：
        1. ``build_fts_query``：jieba 分词 → 每个 token 双引号包裹 → 空格连接
        2. ``tokenize_chinese``：返回分词列表（便于前端高亮 + 调试）
        3. ``top_k`` / ``mode`` 原样回显，调用方按这些参数执行实际查询

    安全：所有 token 经双引号包裹，防止 FTS5 语法注入（如 ``OR``、``*``、``NOT``）。
    """
    fts_query = build_fts_query(req.query)
    # tokenize_chinese 返回空格连接的 token 串，split 还原为列表
    tokens = [t for t in tokenize_chinese(req.query).split(" ") if t]

    return SearchResponse(
        fts_query=fts_query,
        tokens=tokens,
        top_k=req.top_k,
        mode=req.mode,
    )
