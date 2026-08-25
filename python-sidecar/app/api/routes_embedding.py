"""Embedding 模型管理路由（API 规格书 §3.6）。

本任务（T2.6）范围：
  - ``GET  /embedding/models``        列出可用模型（来自注册表）
  - ``POST /embedding/switch``        切换预检查（返回预估信息，状态不改）
  - ``GET  /embedding/rebuild/status``  查询重建进度（内存状态服务）

真实切换动作、embedding 重算长任务留给 T3.x：IPC confirm_embedding_rebuild →
Rust 驱动分批调 Sidecar 长任务接口 + Tauri Event 推进度。
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, Query

from app import state
from app.core import embedding_models
from app.models import (
    EmbeddingModelsResponse,
    EmbeddingSwitchRequest,
    RebuildStatusResponse,
)
from app.services import embedding_switch_service, rebuild_status_service

router = APIRouter(prefix="/embedding", tags=["Embedding"])


@router.get("/models", response_model=EmbeddingModelsResponse)
async def list_models() -> EmbeddingModelsResponse:
    """列出可用的 Embedding 模型（从注册表读取，避免硬编码）。"""
    return EmbeddingModelsResponse(models=embedding_models.list_model_names())


@router.post("/switch", response_model=dict[str, object])
async def switch_model(body: EmbeddingSwitchRequest) -> dict[str, object]:
    """切换预检查：返回是否需要全量重建 + 文件数/耗时/新表名（API §s3-6a）。

    仅计算，不修改任何状态。调用方（Rust IPC）确认后再触发真实切换与重建长任务。
    """
    # 对未知模型抛 ValueError → HTTP 400
    try:
        current_version = state.get_current_embedding_version()
        result = embedding_switch_service.precheck_switch(
            new_model=body.new_model,
            current_model=body.current_model,
            indexed_files=body.indexed_files,
            lancedb=state.get_lancedb(),
            # SC-m2：None 时不再静默回退新模型默认版本（误导 precheck 判定无需重建），
            # 传 0 让 precheck 自然检测到 version 不匹配并触发重建
            current_version=current_version or 0,
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc

    return {"success": True, "data": result.model_dump()}


@router.get("/rebuild/status", response_model=dict[str, object])
async def rebuild_status(
    task_id: str = Query(..., description="重建任务 ID（create_task 返回的 UUID）"),
) -> dict[str, object]:
    """查询重建进度（API §s3-6b）。"""
    task = rebuild_status_service.get_task(task_id)
    if task is None:
        raise HTTPException(status_code=404, detail=f"task {task_id} not found")

    resp = RebuildStatusResponse(
        task_id=task.task_id,
        status=task.status,
        done=task.done,
        total=task.total,
        current_file=task.current_file,
        est_remaining_minutes=rebuild_status_service.est_remaining_minutes(task),
        can_pause=rebuild_status_service.can_pause(task),
        can_resume=rebuild_status_service.can_resume(task),
    )
    return {"success": True, "data": resp.model_dump()}
