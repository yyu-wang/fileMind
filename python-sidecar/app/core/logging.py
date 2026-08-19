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


def _format_kv(msg: str, kwargs: dict[str, object]) -> str:
    """把结构化 kv 参数格式化为消息后缀（stdlib 无 structlog 时的降级）。

    标准库 logging 不接受 ``logger.warning(msg, error=...)`` 这类任意 kwargs，
    此处把字段拼进消息，保证 SidecarLogger Protocol 的行为一致。
    """
    if not kwargs:
        return msg
    pairs = " ".join(f"{key}={value!r}" for key, value in kwargs.items())
    return f"{msg} ({pairs})"


class _StdlibLogger:
    """标准库 logging 适配器：接受 SidecarLogger Protocol 的 ``**kwargs`` 调用。"""

    def __init__(self, logger: logging.Logger) -> None:
        self._logger = logger

    def info(self, msg: str, **kwargs: object) -> None:
        self._logger.info(_format_kv(msg, kwargs))

    def warning(self, msg: str, **kwargs: object) -> None:
        self._logger.warning(_format_kv(msg, kwargs))


def getLogger(name: str = "filemind.sidecar") -> SidecarLogger:  # noqa: N802 - 复刻 logging.getLogger 命名
    """返回 structlog 绑定 logger（有依赖）或标准 logging 适配器。"""
    if _HAS_STRUCTLOG:
        # structlog.get_logger 返回 BoundLogger，与 SidecarLogger Protocol 鸭子兼容
        return structlog.get_logger(name)  # type: ignore[return-value]
    return _StdlibLogger(logging.getLogger(name))
