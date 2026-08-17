"""HMAC 签名验证中间件：对每个非豁免路由验签 + 检查序号防重放。

安全映射：T-01（Sidecar 通信篡改）。

每个非豁免请求必须附带两个头：
- ``X-Signature``: ``HMAC-SHA256(PSK, "{method}|{path}|{body}|{seq}")`` 的 hex
- ``X-Request-Seq``: 单调递增的整数序号，防止重放攻击

豁免路由（``/handshake``、``/health``、文档路由）跳过验签；
dev 模式下（PSK 未设置）所有路由跳过验签，仅本机测试用。
"""

from __future__ import annotations

import hashlib
import hmac
from typing import TYPE_CHECKING

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.responses import JSONResponse

from app import state

if TYPE_CHECKING:
    from collections.abc import Awaitable, Callable

    from starlette.requests import Request
    from starlette.responses import Response

# 豁免路由：握手前必须可访问，/health 用于存活探测
EXEMPT_PATHS: set[str] = {"/handshake", "/health", "/docs", "/openapi.json", "/redoc"}


def _sign(psk: bytes, message: str) -> str:
    """计算 HMAC-SHA256 签名（hex 编码）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _verify(psk: bytes, message: str, signature_hex: str) -> bool:
    """常量时间验证签名（防时序攻击）。"""
    expected = _sign(psk, message)
    return hmac.compare_digest(expected, signature_hex)


def _build_request_canonical(method: str, path: str, body: str, seq: int) -> str:
    """构造请求签名 canonical string：``{method}|{path}|{body}|{seq}``。"""
    return f"{method}|{path}|{body}|{seq}"


def _error_response(status_code: int, detail: str) -> JSONResponse:
    """构造标准错误响应。"""
    return JSONResponse(status_code=status_code, content={"detail": detail})


class HMACMiddleware(BaseHTTPMiddleware):
    """HMAC 验签 + 序号防重放中间件。"""

    async def dispatch(
        self,
        request: Request,
        call_next: Callable[[Request], Awaitable[Response]],
    ) -> Response:
        path = request.url.path

        # 豁免路由直接放行
        if path in EXEMPT_PATHS:
            return await call_next(request)

        psk = state.get_psk()

        # dev 模式（PSK 未设置）：跳过验签，仅本机测试用
        if psk is None:
            return await call_next(request)

        # 提取签名头
        signature = request.headers.get("X-Signature")
        seq_str = request.headers.get("X-Request-Seq")
        if not signature or not seq_str:
            return _error_response(401, "SEC-E-002:缺少签名头")

        # 解析序号
        try:
            seq = int(seq_str)
        except ValueError:
            return _error_response(401, "SEC-E-002:序号格式无效")

        # 序号防重放：必须严格递增
        if seq <= state.get_last_seq():
            return _error_response(401, "SEC-E-002:序号重放或乱序")

        # 读取请求体（POST/PUT/PATCH 才有 body，GET 为空）
        body_bytes = await request.body()
        body = body_bytes.decode("utf-8") if body_bytes else ""

        # 验签
        canonical = _build_request_canonical(request.method, path, body, seq)
        if not _verify(psk, canonical, signature):
            return _error_response(401, "SEC-E-002:签名验证失败")

        # 验签通过，更新 last_seq
        state.set_last_seq(seq)

        return await call_next(request)
