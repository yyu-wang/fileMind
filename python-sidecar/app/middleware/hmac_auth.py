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
import os
from typing import TYPE_CHECKING

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.responses import JSONResponse

from app import state

if TYPE_CHECKING:
    from collections.abc import Awaitable, Callable

    from starlette.requests import Request
    from starlette.responses import Response

# 豁免路由：握手前必须可访问，/health 用于存活探测
# SC-m4：/docs /openapi.json /redoc 不再无条件豁免（信息暴露）；dev 模式可用 FILEMIND_DEV=1 恢复
EXEMPT_PATHS: set[str] = {"/handshake", "/health"}
if os.environ.get("FILEMIND_DEV", "0") == "1":
    EXEMPT_PATHS |= {"/docs", "/openapi.json", "/redoc"}

# SC-m3：请求体大小上限（64MB；正常 JSON 请求体远小于此，
# 放宽以容纳 RAG 问答注入的 FTS 命中大文件正文，见 chat.rs MAX_CHAT_FTS_BYTES）
MAX_BODY_SIZE = 64 * 1024 * 1024


def _sign(psk: bytes, message: str) -> str:
    """计算 HMAC-SHA256 签名（hex 编码）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _verify(psk: bytes, message: str, signature_hex: str) -> bool:
    """常量时间验证签名（防时序攻击）。

    两侧统一 encode 为 bytes 再比较：``compare_digest`` 收到非 ASCII 的
    str 会抛 TypeError，恶意构造的签名头不应把验签打成 500（SC-M1）。
    """
    expected = _sign(psk, message)
    return hmac.compare_digest(expected.encode("utf-8"), signature_hex.encode("utf-8"))


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

        # 解析序号：严格 ASCII 十进制。int() 会接受 "+5"/" 5"/全角数字等
        # 宽松格式，canonical 两侧不一致会造成验签语义混乱（SC-M1）
        if not (seq_str.isascii() and seq_str.isdigit()):
            return _error_response(401, "SEC-E-002:序号格式无效")
        seq = int(seq_str)

        # SC-m4：OPTIONS 预检直接放行（CORS 在 HMAC 之后，OPTIONS 无签名头会 401）
        if request.method == "OPTIONS":
            return await call_next(request)

        # 先读请求体（POST/PUT/PATCH 才有 body，GET 为空）。
        # 这是本函数在 seq 校验前唯一的挂起点：必须放在原子段之外，
        # 否则 await 期间事件循环可切走执行并发请求（SC-M1 竞态根源）
        body_bytes = await request.body()
        # SC-m3：body 大小上限 1MB（超限拒绝防资源耗尽）
        if len(body_bytes) > MAX_BODY_SIZE:
            return _error_response(413, "SEC-E-002:请求体过大")
        # SC-m3：decode 加保护——恶意非 UTF-8 body 不应打成 500
        try:
            body = body_bytes.decode("utf-8") if body_bytes else ""
        except UnicodeDecodeError:
            return _error_response(400, "SEC-E-002:请求体非 UTF-8")

        # ---- 原子段：seq 读取 → 判断 → 验签 → 写入，中间无 await ----
        # asyncio 单线程模型下，无挂起点即不会被并发协程插入，
        # 两个同 seq 的并发请求只有一个能通过检查并写入 last_seq
        if seq <= state.get_last_seq():
            return _error_response(401, "SEC-E-002:序号重放或乱序")

        canonical = _build_request_canonical(request.method, path, body, seq)
        if not _verify(psk, canonical, signature):
            return _error_response(401, "SEC-E-002:签名验证失败")

        state.set_last_seq(seq)
        # ---- 原子段结束 ----

        return await call_next(request)
