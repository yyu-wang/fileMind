"""日志脱敏（安全 I-03）单元测试。

覆盖：
1. ``sanitize_path``：POSIX / Windows 分隔符与边界。
2. ``redact``：API Key、Bearer Token、key=value、POSIX / Windows 绝对路径；
   URL 与单段系统路径不受影响；普通文本不被误伤。
3. ``_MaskingLogger``：作为 getLogger 单一出口，对任意底层 logger（structlog /
   stdlib 适配器）的 msg 与字符串 kv 值统一脱敏。
4. ``getLogger``（stdlib 回退路径）：实际输出文本不含明文路径与 API Key。
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING

from app.core import logging as logging_mod
from app.core.logging import _MaskingLogger, getLogger, redact, sanitize_path

if TYPE_CHECKING:
    import pytest


class _CaptureHandler(logging.Handler):
    """捕获 stdlib 日志记录的测试 handler。"""

    def __init__(self, records: list[logging.LogRecord]) -> None:
        super().__init__()
        self._records = records

    def emit(self, record: logging.LogRecord) -> None:
        self._records.append(record)


def test_sanitize_path_handles_separators_and_edges() -> None:
    assert sanitize_path("/Users/x/docs") == "***/docs"
    assert sanitize_path(r"C:\a\b\c.txt") == "***/c.txt"
    assert sanitize_path("/a/b/") == "***/b"
    assert sanitize_path("/") == "***"
    assert sanitize_path("") == "***"


def test_redact_masks_api_key() -> None:
    msg = "请求失败 key=sk-abcdefghijklmnopqrstuvwxyz123456"
    out = redact(msg)
    assert "***REDACTED***" in out
    assert "sk-abcdefghijklmnopqrstuvwxyz123456" not in out


def test_redact_masks_bearer_token() -> None:
    msg = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature_1"
    out = redact(msg)
    assert "***REDACTED***" in out
    assert "eyJhbGciOiJIUzI1NiJ9" not in out


def test_redact_masks_key_value_forms() -> None:
    msg = "api_key=supersecret12345, password = hunter2value, secret: mysecret999"
    out = redact(msg)
    assert "supersecret12345" not in out
    assert "hunter2value" not in out
    assert "mysecret999" not in out
    assert out.count("***REDACTED***") == 3


def test_redact_masks_absolute_paths() -> None:
    posix = redact("处理完成: /Users/wangyu/Desktop/report.pdf")
    assert "***/report.pdf" in posix
    assert "/Users/wangyu" not in posix

    windows = redact(r"读取失败: C:\Users\wangyu\docs\a.txt")
    assert "***/a.txt" in windows
    assert r"C:\Users\wangyu" not in windows


def test_redact_preserves_url_and_single_segment_paths() -> None:
    msg = "sidecar http://127.0.0.1:8765/health 检查 /tmp 目录"
    assert redact(msg) == msg


def test_redact_preserves_ordinary_text() -> None:
    msg = "扫描完成 files=42 duration_ms=350 mode=local"
    assert redact(msg) == msg


class _FakeInner:
    """记录调用的假底层 logger（模拟 structlog BoundLogger）。"""

    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, object]]] = []

    def info(self, msg: str, **kwargs: object) -> None:
        self.calls.append((msg, kwargs))

    def warning(self, msg: str, **kwargs: object) -> None:
        self.calls.append((msg, kwargs))


def test_masking_logger_wraps_any_inner() -> None:
    inner = _FakeInner()
    wrapped = _MaskingLogger(inner)
    wrapped.info("/Users/x/a.txt 已处理", path="/Users/x/b.txt", ok=True)

    (msg, kwargs) = inner.calls[0]
    assert "***/a.txt" in msg
    assert kwargs["path"] == "***/b.txt"
    assert kwargs["ok"] is True


def test_getlogger_fallback_masks_output(monkeypatch: pytest.MonkeyPatch) -> None:
    """强制 stdlib 回退路径，验证实际输出文本不含明文路径/Key。"""
    monkeypatch.setattr(logging_mod, "_HAS_STRUCTLOG", False)

    logger = getLogger("filemind.masktest")
    records: list[logging.LogRecord] = []
    handler = _CaptureHandler(records)
    stdlib_logger = logging.getLogger("filemind.masktest")
    stdlib_logger.handlers.clear()
    stdlib_logger.setLevel(logging.INFO)
    stdlib_logger.addHandler(handler)
    stdlib_logger.propagate = False

    try:
        logger.info(
            "开始扫描: /Users/wangyu/Desktop/secret.pdf",
            path="/Users/wangyu/docs/x.txt",
            api_key="sk-abcdefghijklmnopqrstuvwxyz123456",
            file_id=42,
        )
    finally:
        stdlib_logger.removeHandler(handler)

    text = "\n".join(record.getMessage() for record in records)
    assert "/Users/wangyu" not in text
    assert "sk-abcdefghijklmnopqrstuvwxyz123456" not in text
    assert "***/secret.pdf" in text
    assert "***/x.txt" in text
    assert "file_id=42" in text
