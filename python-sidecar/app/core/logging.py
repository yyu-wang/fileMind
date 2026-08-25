"""Sidecar 日志适配：优先 structlog（生产带 kv 结构化日志），否则回退 logging。

统一在 ``getLogger`` 出口做日志脱敏（安全 I-03）：msg 与所有字符串 kv 值经
``redact`` 过滤后再交给底层 logger，保证新增日志语句即使忘记手动消毒也不会
泄漏 API Key / 绝对路径。

不对外暴露三方依赖，避免打包态 PyInstaller 缺 structlog 时直接 ImportError 起不来。
"""

from __future__ import annotations

import logging
import re
from typing import Protocol, runtime_checkable

try:
    import structlog  # type: ignore[import-not-found]

    _HAS_STRUCTLOG = True
except Exception:  # noqa: BLE001 - dev venv / ci 可能没有装，fallback 到标准 logging
    _HAS_STRUCTLOG = False


# —— 日志脱敏（与 Rust src-tauri/src/security/log_redact.rs 规则保持一致）——

_REDACTED = "***REDACTED***"
_PATH_PREFIX = "***"

# 密钥/令牌型泄漏：顺序敏感，须在路径处理前整体替换（token 可含 `/`）
_KEY_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"sk-[A-Za-z0-9_-]{16,}"),
    re.compile(r"(?i)bearer[ =:]+[A-Za-z0-9._~+/=-]{8,}"),
    re.compile(
        r"(?i)(?:api[_-]?key|access[_-]?token|secret|password)[ =:]+[\"']?[A-Za-z0-9._~+/=-]{6,}"
    ),
)

# SC-m26：URL 模式——先保护 URL，避免路径正则误匹配 URL 中的路径段
_URL_PATTERN = re.compile(r'https?://[^\s"\'(),;:]+')

# 绝对路径型：POSIX（≥2 段）+ Windows 盘符两种写法。段内排除空白与常见标点，
# 避免跨词贪心；含空格的路径由调用侧 sanitize_path 兜底。
_PATH_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"/(?:[^/\s\"'(),;:]+/)+[^/\s\"'(),;:]+"),
    re.compile(r"[A-Za-z]:\\(?:[^\\\s\"'(),;:]+)(?:\\[^\\\s\"'(),;:]+)*"),
    re.compile(r"[A-Za-z]:/(?:[^/\s\"'(),;:]+)(?:/[^/\s\"'(),;:]+)+"),
)


def sanitize_path(path: str) -> str:
    """路径脱敏：仅保留最后一级，前缀替换为 `***`（与 Rust sanitize_path 一致）。"""
    normalized = path.replace("\\", "/")
    segments = [seg for seg in normalized.split("/") if seg]
    if not segments:
        return _PATH_PREFIX
    return f"{_PATH_PREFIX}/{segments[-1]}"


def redact(text: str) -> str:
    """把文本中的 API Key / 绝对路径替换为占位符。

    先处理密钥/令牌（可能含 `/`），再把绝对路径收敛为 `***/末级`。用户查询内容属
    自由文本，无法靠正则判定；规范要求「不记录原始内容」，由调用侧保证不落日志。
    """
    for pattern in _KEY_PATTERNS:
        text = pattern.sub(_REDACTED, text)
    # SC-m26：URL 先保护——路径正则会误匹配 URL 中的 /path 段
    urls = _URL_PATTERN.findall(text)
    placeholders: dict[str, str] = {}
    for i, url in enumerate(urls):
        ph = f"\x00URL{i}\x00"
        placeholders[ph] = url
        text = text.replace(url, ph)
    for pattern in _PATH_PATTERNS:
        text = pattern.sub(lambda m: sanitize_path(m.group(0)), text)
    for ph, url in placeholders.items():
        text = text.replace(ph, url)
    return text


@runtime_checkable
class SidecarLogger(Protocol):
    """Logger duck-type：.info/.warning/.error/.exception/.debug 是承诺的 API。"""

    def info(self, msg: str, **kwargs: object) -> None: ...
    def warning(self, msg: str, **kwargs: object) -> None: ...
    def error(self, msg: str, **kwargs: object) -> None: ...
    def exception(self, msg: str, **kwargs: object) -> None: ...
    def debug(self, msg: str, **kwargs: object) -> None: ...


class _MaskingLogger:
    """统一脱敏包装：msg 与所有字符串 kv 值经 redact 过滤后再交给底层 logger。

    作为 ``getLogger`` 的单一出口，structlog 与 stdlib 两条路径共用，保证新增日志
    语句即使忘记手动消毒也不会泄漏敏感信息。
    """

    def __init__(self, inner: SidecarLogger) -> None:
        self._inner = inner

    def info(self, msg: str, **kwargs: object) -> None:
        self._inner.info(redact(msg), **self._mask_kwargs(kwargs))

    def warning(self, msg: str, **kwargs: object) -> None:
        self._inner.warning(redact(msg), **self._mask_kwargs(kwargs))

    # SC-m26：补 error/exception/debug——原仅 info/warning 脱敏，error 日志会泄漏
    def error(self, msg: str, **kwargs: object) -> None:
        self._inner.error(redact(msg), **self._mask_kwargs(kwargs))

    def exception(self, msg: str, **kwargs: object) -> None:
        self._inner.exception(redact(msg), **self._mask_kwargs(kwargs))

    def debug(self, msg: str, **kwargs: object) -> None:
        self._inner.debug(redact(msg), **self._mask_kwargs(kwargs))

    @staticmethod
    def _mask_kwargs(kwargs: dict[str, object]) -> dict[str, object]:
        return {
            key: redact(value) if isinstance(value, str) else value for key, value in kwargs.items()
        }


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

    # SC-m26：补 error/exception/debug
    def error(self, msg: str, **kwargs: object) -> None:
        self._logger.error(_format_kv(msg, kwargs))

    def exception(self, msg: str, **kwargs: object) -> None:
        self._logger.exception(_format_kv(msg, kwargs))

    def debug(self, msg: str, **kwargs: object) -> None:
        self._logger.debug(_format_kv(msg, kwargs))


def getLogger(name: str = "filemind.sidecar") -> SidecarLogger:  # noqa: N802 - 复刻 logging.getLogger 命名
    """返回统一脱敏包装后的 logger（structlog 或标准 logging 适配器）。"""
    if _HAS_STRUCTLOG:
        # structlog.get_logger 返回 BoundLogger，与 SidecarLogger Protocol 鸭子兼容
        return _MaskingLogger(structlog.get_logger(name))  # type: ignore[arg-type]
    return _MaskingLogger(_StdlibLogger(logging.getLogger(name)))
