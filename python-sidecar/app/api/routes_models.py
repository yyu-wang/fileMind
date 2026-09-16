"""T2 — Embedding 模型下载路由（API 约定见 T2 计划）+ T1 离线模型包导入。

端点：
  - ``GET  /models/download/status``  查询下载状态（进度 / 当前镜像 / 尝试次数 / 失败原因）
  - ``POST /models/download``         启动下载（幂等；``failed`` 后再次调用即用户手动重试）
  - ``POST /models/import``           导入离线模型包（内网 / 无外网部署的模型分发通道）

下载在后台任务中进行，接口**立即返回当前状态**；调用方（Rust IPC → 设置页）
按固定间隔轮询状态接口渲染进度条。下载失败不会让接口 5xx——失败原因通过
``status=failed`` + ``error`` 字段结构化返回，与 ``/inference/test`` 同一风格。

导入为**同步阻塞**操作（本地拷贝，最大模型 2.2GB），故接口等到导入结束后一次性返回
结果：包不合法要在 UI 上立刻给出「缺哪个文件」，异步化只会把失败变成稍后才出现的
谜题。错误码见 ``rules/error-handling.md``（EMB-V-001 内容不符合要求 / EMB-U-002 导入失败）。
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, Query

from app.models import (
    ModelDownloadRequest,
    ModelDownloadStatusResponse,
    ModelImportRequest,
    ModelImportResponse,
)
from app.services import model_download_service, model_import_service

router = APIRouter(prefix="/models", tags=["模型"])


@router.get("/download/status", response_model=ModelDownloadStatusResponse)
async def download_status(
    model_name: str = Query(
        ..., description="模型名（Embedding 注册表名如 bge-large-zh-v1.5；或 bge-reranker-v2-m3）"
    ),
) -> ModelDownloadStatusResponse:
    """查询指定模型的下载状态。

    Args:
        model_name: 模型名（Embedding 注册表名，或重排模型名）。

    Returns:
        当前状态；未开始且本地文件已齐备时返回 ``status=ready``。

    Raises:
        HTTPException 400: 模型名不在任一模型来源中。
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
        HTTPException 400: 模型名不在任一模型来源中。
    """
    try:
        return await model_download_service.ensure_downloaded(body.model_name)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post("/import", response_model=ModelImportResponse)
async def import_models(body: ModelImportRequest) -> ModelImportResponse:
    """导入离线模型包（zip 或 models 目录），补齐本机模型文件。

    路径由 Rust 侧经 ``path_guard::validate()`` 校验后传入（Sidecar 不自行放宽校验），
    本接口只做包内容校验与落盘。

    Args:
        body: 含 ``path``（离线包路径）。

    Returns:
        导入结果：``imported``（本次写入的模型）+ ``skipped``（本机已就绪跳过的模型）。

    Raises:
        HTTPException 422: 包内容不符合要求（无可用模型 / 模型目录缺文件）——EMB-V-001。
        HTTPException 500: 包不可读或落盘失败——EMB-U-002。
    """
    try:
        outcome = await model_import_service.import_package(body.path)
    except model_import_service.PackageInvalidError as exc:
        raise HTTPException(status_code=422, detail=f"EMB-V-001:{exc}") from exc
    except model_import_service.PackageSourceError as exc:
        raise HTTPException(status_code=500, detail=f"EMB-U-002:模型导入失败（{exc}）") from exc
    return ModelImportResponse(imported=outcome.imported, skipped=outcome.skipped)
