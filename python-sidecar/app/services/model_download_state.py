"""下载状态模型与错误类型（原 ``model_download_service.py`` 拆出）。

本模块是下载链路的**叶子依赖**：状态数据类、状态词表与时间戳助手被
``model_download_service``（编排与公开 API）与 ``model_download_transfer``
（HTTP 传输）共同引用，独立成文件后两者都只单向依赖本模块，不形成环。
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field

from app.models import ModelDownloadStatusResponse

#: 状态语义（``status``）：``idle`` 未开始 / ``downloading`` 进行中 /
#: ``ready`` 文件齐备 / ``failed`` 自动重试用尽
_STATUS_IDLE = "idle"
_STATUS_DOWNLOADING = "downloading"
_STATUS_READY = "ready"
_STATUS_FAILED = "failed"


class ModelDownloadError(Exception):
    """下载内容异常（内容为空等）：由 ``_download_all`` 捕获后切换镜像重试。"""


def _now_iso() -> str:
    """当前时间的 ISO 8601 字符串（秒精度）。"""
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime())


@dataclass
class _State:
    """单个模型的下载状态（进程内，可变）。"""

    model_name: str
    status: str = _STATUS_IDLE
    mirror: str | None = None
    attempt: int = 0
    downloaded_bytes: int = 0
    total_bytes: int | None = None
    error: str | None = None
    updated_at: str = ""
    #: 进度节流用的最近一次上报时间（monotonic），不对外暴露
    last_report: float = field(default=0.0, repr=False)

    def as_response(self) -> ModelDownloadStatusResponse:
        """转换为对外响应模型。"""
        return ModelDownloadStatusResponse(
            model_name=self.model_name,
            status=self.status,
            mirror=self.mirror,
            attempt=self.attempt,
            downloaded_bytes=self.downloaded_bytes,
            total_bytes=self.total_bytes,
            error=self.error,
            updated_at=self.updated_at,
        )
