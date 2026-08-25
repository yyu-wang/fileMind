import time

from fastapi import APIRouter

from app.models import HealthResponse

router = APIRouter(prefix="/health", tags=["健康检查"])

# SC-m1：模块导入时即记录启动时间（原 None+首次请求设值导致首问 uptime≈0）
_START_TIME = time.time()


def _get_uptime() -> float:
    return time.time() - _START_TIME


@router.get("", response_model=HealthResponse)
async def health_check() -> HealthResponse:
    """检查 Sidecar 服务健康状态。"""
    return HealthResponse(
        status="ok",
        version="0.1.0",
        uptime_seconds=_get_uptime(),
    )
