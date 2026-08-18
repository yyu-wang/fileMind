"""Sidecar 进程内存监控路由：供 Go/No-Go 第 7 项（内存 <300MB）验收。

端点 GET /metrics 返回当前进程 RSS/VMS 字节数换算的 MB 值 + 是否在验收阈值内。
阈值 300MB 来自 ``10_开发任务拆解与排期.html`` E1 Epic 门控定义；
1GB 运行期硬保护（见 ``07_安全合规设计.html`` 第 389 行）在 T7 安全
合规阶段实现，T1.6 只做采集不做拒绝。
"""

from __future__ import annotations

import psutil  # type: ignore[import-untyped]
from fastapi import APIRouter

from app.models import MetricsResponse

router = APIRouter(prefix="/metrics", tags=["监控"])

# Go/No-Go 门控线：Sidecar 进程内存 <300MB（10_开发任务拆解与排期 第 380 行）。
GNG_MEMORY_THRESHOLD_MB: int = 300


@router.get("", response_model=MetricsResponse)
async def get_metrics() -> MetricsResponse:
    """读取 Sidecar 自身进程 RSS/VMS，换算 MB 并与阈值比较。

    Returns:
        MetricsResponse — 四字段：``rss_mb`` 物理内存、``vms_mb`` 虚拟内存、
        ``threshold_mb=300``、``within_limit`` 是否通过 Go/No-Go 门控。

    Security:
        本路由**不**加入 HMAC 豁免路径（仅 Rust 端持有 PSK），防止未授权
        第三方探测进程内存指纹与运行期状态（T-01 安全延伸）。
    """
    process = psutil.Process()
    mem = process.memory_info()
    rss_mb = round(mem.rss / 1024 / 1024, 2)
    vms_mb = round(mem.vms / 1024 / 1024, 2)
    return MetricsResponse(
        rss_mb=rss_mb,
        vms_mb=vms_mb,
        threshold_mb=GNG_MEMORY_THRESHOLD_MB,
        within_limit=rss_mb < GNG_MEMORY_THRESHOLD_MB,
    )
