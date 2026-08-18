"""Sidecar 日志适配：优先 structlog（生产带 kv 结构化日志），否则回退 logging。

不对外暴露三方依赖，避免打包态 PyInstaller 缺 structlog 时直接 ImportError 起不来。
"""

from __future__ import annotations

import logging
from typing import Protocol, runtime_checkable

try:
    import structlog  # type: ignore[import-not-found]

    _HAS_STRUCTLOG = True
except Exception:  # noqa: BLE001 - dev venv / ci 可能没有装，fallback 到标准 logging
    _HAS_STRUCTLOG = False


@runtime_checkable
class SidecarLogger(Protocol):
    """Logger duck-type：.info/.warning(msg, **kwargs) 是唯一承诺的 API。"""

    def info(self, msg: str, **kwargs: object) -> None: ...
    def warning(self, msg: str, **kwargs: object) -> None: ...


def getLogger(name: str = "filemind.sidecar") -> SidecarLogger:  # noqa: N802 - 复刻 logging.getLogger 命名
    """返回 structlog 绑定 logger（有依赖）或标准 logging.Logger。"""
    if _HAS_STRUCTLOG:
        # structlog.get_logger 返回 BoundLogger，与 SidecarLogger Protocol 鸭子兼容
        return structlog.get_logger(name)  # type: ignore[return-value]
    # 标准 logging.Logger 与 Protocol 的方法签名存在参数变长差异（*args vs **kwargs），
    # 但运行时鸭子类型兼容；用 ignore 静音静态检查
    return logging.getLogger(name)  # type: ignore[return-value]
