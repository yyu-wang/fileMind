"""T2 — Embedding 模型下载路由（API 约定见 T2 计划）。

端点：
  - ``GET  /models/download/status``  查询下载状态（进度 / 当前镜像 / 尝试次数 / 失败原因）
  - ``POST /models/download``         启动下载（幂等；``failed`` 后再次调用即用户手动重试）

下载在后台任务中进行，接口**立即返回当前状态**；调用方（Rust IPC → 设置页）
按固定间隔轮询状态接口渲染进度条。下载失败不会让接口 5xx——失败原因通过
``status=failed`` + ``error`` 字段结构化返回，与 ``/inference/test`` 同一风格。
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, Query

from app.models import ModelDownloadRequest, ModelDownloadStatusResponse
from app.services import model_download_service

router = APIRouter(prefix="/models", tags=["模型"])


@router.get("/download/status", response_model=ModelDownloadStatusResponse)
async def download_status(
    model_name: str = Query(..., description="注册表模型名（如 bge-large-zh-v1.5）"),
) -> ModelDownloadStatusResponse:
    """查询指定模型的下载状态。

    Args:
        model_name: 注册表模型名。

    Returns:
        当前状态；未开始且本地文件已齐备时返回 ``status=ready``。

    Raises:
        HTTPException 400: 模型名不在注册表中。
    """
    try:
        return model_download_service.get_status(model_name)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post("/download", response_model=ModelDownloadStatusResponse)
async def start_download(body: ModelDownloadRequest) -> ModelDownloadStatusResponse:
    """启动（或手动重试）模型下载。

    Args:
        body: 含 ``model_name``。

    Returns:
        启动后的当前状态（``downloading`` / 已就绪则为 ``ready``）。

    Raises:
        HTTPException 400: 模型名不在注册表中。
    """
    try:
        return await model_download_service.ensure_downloaded(body.model_name)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
