from fastapi import APIRouter

from app.models import HealthResponse

router = APIRouter(prefix="/health", tags=["健康检查"])

_START_TIME = None


def _get_uptime() -> float:
    import time

    global _START_TIME
    if _START_TIME is None:
        _START_TIME = time.time()
    return time.time() - _START_TIME


@router.get("", response_model=HealthResponse)
async def health_check() -> HealthResponse:
    """检查 Sidecar 服务健康状态。"""
    return HealthResponse(
        status="ok",
        version="0.1.0",
        uptime_seconds=_get_uptime(),
    )
