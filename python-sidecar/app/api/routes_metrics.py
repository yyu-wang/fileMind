"""Sidecar 进程内存监控路由：供 Go/No-Go 第 7 项（内存 <500MB）验收。

端点 GET /metrics 返回当前进程 RSS/VMS 字节数换算的 MB 值 + 是否在验收阈值内。
阈值 500MB 是 T10.3（E10 性能优化）放宽的门控：原 300MB 无法容纳 rerank
懒加载模型的冷启动余量，500MB 仍守住「常驻不预载 rerank」的内存优先取舍
（模型首次检索时才加载，见 ``rerank_service``）。
env ``FILEMIND_MEMORY_THRESHOLD_MB`` 可覆盖阈值（非法值回落默认）。
1GB 运行期硬保护（见 ``07_安全合规设计.html`` 第 389 行）在 T7 安全
合规阶段实现，T1.6 只做采集不做拒绝。
"""

from __future__ import annotations

import os

import psutil  # type: ignore[import-untyped]
from fastapi import APIRouter

from app.models import MetricsResponse

router = APIRouter(prefix="/metrics", tags=["监控"])

#: 默认 Go/No-Go 门控线：Sidecar 冷启动内存 <500MB（T10.3 从 300MB 放宽）。
DEFAULT_MEMORY_THRESHOLD_MB = 500
#: 环境变量：门控阈值覆盖（正整数，非法值回落默认）
_ENV_THRESHOLD_MB = "FILEMIND_MEMORY_THRESHOLD_MB"


def memory_threshold_mb() -> int:
    """内存门控阈值：默认 500，env ``FILEMIND_MEMORY_THRESHOLD_MB`` 可覆盖。

    非法值（非整数 / 非正数）回落默认，避免配置错误导致门控失效。
    """
    raw = os.environ.get(_ENV_THRESHOLD_MB, "")
    try:
        value = int(raw)
    except ValueError:
        return DEFAULT_MEMORY_THRESHOLD_MB
    return value if value > 0 else DEFAULT_MEMORY_THRESHOLD_MB


@router.get("", response_model=MetricsResponse)
async def get_metrics() -> MetricsResponse:
    """读取 Sidecar 自身进程 RSS/VMS，换算 MB 并与阈值比较。

    Returns:
        MetricsResponse — 四字段：``rss_mb`` 物理内存、``vms_mb`` 虚拟内存、
        ``threshold_mb=500``、``within_limit`` 是否通过 Go/No-Go 门控。

    Security:
        本路由**不**加入 HMAC 豁免路径（仅 Rust 端持有 PSK），防止未授权
        第三方探测进程内存指纹与运行期状态（T-01 安全延伸）。
    """
    threshold = memory_threshold_mb()
    process = psutil.Process()
    mem = process.memory_info()
    rss_mb = round(mem.rss / 1024 / 1024, 2)
    vms_mb = round(mem.vms / 1024 / 1024, 2)
    return MetricsResponse(
        rss_mb=rss_mb,
        vms_mb=vms_mb,
        threshold_mb=threshold,
        within_limit=rss_mb < threshold,
    )
