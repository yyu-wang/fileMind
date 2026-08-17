"""Sidecar 优雅关闭路由：由 Rust 端在退出时调用。

Rust 端流程：先 POST /shutdown（带 HMAC 签名）→ Sidecar 立即返回 200 →
Sidecar 后台 sleep 0.5s 后 ``sys.exit(0)`` 让自身进程正常退出。
这样 Rust 端可以拿到响应确认后进入 kill-wait 兜底阶段，避免杀未就绪进程。
"""

from __future__ import annotations

import asyncio
import sys

from fastapi import APIRouter

from app.models import ShutdownResponse

router = APIRouter(prefix="/shutdown", tags=["生命周期"])


async def _graceful_exit() -> None:
    """后台任务：响应发完后短暂 sleep，然后退出自身进程。

    sleep 0.5 秒用于保证当前响应已被底层 ASGI（uvicorn）完全写出，
    避免 sys.exit 在响应未 flush 前触发 SocketError。
    """
    await asyncio.sleep(0.5)
    sys.exit(0)


@router.post("", response_model=ShutdownResponse)
async def shutdown() -> ShutdownResponse:
    """触发 Sidecar 优雅关闭。

    Returns:
        ``{"status": "shutting_down"}`` 立即返回，真正退出在后台延后 0.5s 执行。

    Notes:
        本路由不豁免 HMAC 中间件：只有 Rust 端持有 PSK 才能调用，
        防止未授权第三方让 Sidecar 误退出（安全 T-01 延伸）。
    """
    # 不 await —— 先返回响应再执行退出逻辑
    asyncio.create_task(_graceful_exit())
    return ShutdownResponse(status="shutting_down")
