"""HMAC 验签中间件单元测试：验签通过/签名缺失/错误拒绝/seq 防重放/豁免路由。

安全映射：T-01（Sidecar 通信篡改）。

测试覆盖：
- 正确签名 + 递增 seq → 中间件放行（下游返回 404 证明到达路由层）
- 缺少 X-Signature / X-Request-Seq → 401
- 错误签名 → 401
- seq 重放或乱序（seq <= last_seq）→ 401
- seq 格式无效（非数字）→ 401
- /health、/docs、/openapi.json 豁免 → 无签名头也放行
- dev 模式（PSK 未设置）所有路由跳过验签

测试目标路径用不存在的 ``/nonexistent``：中间件在路由匹配前执行，
可清晰断言 401（中间件拦截）vs 404（中间件放行后路由未匹配）。
"""

from __future__ import annotations

import asyncio
import hashlib
import hmac
from typing import TYPE_CHECKING

import httpx
import pytest
from fastapi.testclient import TestClient

from app import state
from app.main import app

if TYPE_CHECKING:
    # TestClient 的 HTTP 后端：starlette≥1.6 优先 import httpx2（httpx 的新包名）。
    # 类型上必须跟随，否则 mypy 报 httpx/httpx2 Response 不兼容。
    # 运行时不执行（TYPE_CHECKING），starlette 自带 httpx2→httpx 回退逻辑
    import httpx2 as _testclient_httpx

_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"


def _sign(psk: bytes, message: str) -> str:
    """构造 HMAC-SHA256 hex 签名（与 ``app.middleware.hmac_auth._sign`` 算法一致）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _build_canonical(method: str, path: str, body: str, seq: int) -> str:
    """构造请求 canonical string（与 ``hmac_auth._build_request_canonical`` 一致）。"""
    return f"{method}|{path}|{body}|{seq}"


@pytest.fixture
def client() -> TestClient:
    """每个测试前重置全局状态并注入固定 PSK。"""
    state.reset_state()
    state.set_psk(_TEST_PSK)
    return TestClient(app)


def _signed_get(
    client: TestClient, path: str, seq: int, psk: bytes = _TEST_PSK
) -> _testclient_httpx.Response:
    """携带正确签名 + seq 访问 GET 路由。"""
    canonical = _build_canonical("GET", path, "", seq)
    signature = _sign(psk, canonical)
    return client.get(
        path,
        headers={"X-Signature": signature, "X-Request-Seq": str(seq)},
    )


def test_middleware_verify_pass(client: TestClient) -> None:
    """正确签名 + 递增 seq → 中间件放行（下游返回 404 证明到达路由层）。"""
    response = _signed_get(client, "/nonexistent", seq=1)
    assert response.status_code == 404  # 中间件放行后路由未匹配


def test_middleware_missing_signature(client: TestClient) -> None:
    """缺少 X-Signature 头 → 401。"""
    response = client.get("/nonexistent", headers={"X-Request-Seq": "1"})
    assert response.status_code == 401
    assert "SEC-E-002" in response.json()["detail"]


def test_middleware_missing_seq(client: TestClient) -> None:
    """缺少 X-Request-Seq 头 → 401。"""
    canonical = _build_canonical("GET", "/nonexistent", "", 1)
    signature = _sign(_TEST_PSK, canonical)
    response = client.get("/nonexistent", headers={"X-Signature": signature})
    assert response.status_code == 401
    assert "SEC-E-002" in response.json()["detail"]


def test_middleware_wrong_signature(client: TestClient) -> None:
    """错误签名（全 0）→ 401。"""
    response = client.get(
        "/nonexistent",
        headers={"X-Signature": "0" * 64, "X-Request-Seq": "1"},
    )
    assert response.status_code == 401
    assert "SEC-E-002" in response.json()["detail"]


def test_middleware_seq_replay(client: TestClient) -> None:
    """seq <= last_seq → 401（防重放：新请求 seq 必须严格递增）。"""
    state.set_last_seq(5)  # 预设 last_seq，使 seq=5 不严格大于
    response = _signed_get(client, "/nonexistent", seq=5)
    assert response.status_code == 401
    assert "SEC-E-002" in response.json()["detail"]


def test_middleware_invalid_seq_format(client: TestClient) -> None:
    """seq 非数字 → 401。"""
    canonical = _build_canonical("GET", "/nonexistent", "", 1)
    signature = _sign(_TEST_PSK, canonical)
    response = client.get(
        "/nonexistent",
        headers={"X-Signature": signature, "X-Request-Seq": "not-a-number"},
    )
    assert response.status_code == 401
    assert "SEC-E-002" in response.json()["detail"]


def test_middleware_health_exempt(client: TestClient) -> None:
    """/health 在 EXEMPT_PATHS 中 → 无签名头也放行。"""
    response = client.get("/health")
    assert response.status_code == 200
    assert response.json()["status"] == "ok"


def test_middleware_docs_not_exempt_by_default(client: TestClient) -> None:
    """SC-m4：/docs 默认不豁免（信息暴露），需 FILEMIND_DEV=1 才放行。"""
    response = client.get("/docs")
    assert response.status_code == 401


def test_middleware_openapi_not_exempt_by_default(client: TestClient) -> None:
    """SC-m4：/openapi.json 默认不豁免（信息暴露）。"""
    response = client.get("/openapi.json")
    assert response.status_code == 401


def test_middleware_dev_mode_skips_verify(client: TestClient) -> None:
    """dev 模式（PSK 未设置）所有非豁免路由跳过验签。

    无签名头访问 /nonexistent → 中间件跳过 → 路由返回 404（而非 401），
    证明 PSK=None 时中间件未执行验签。
    """
    state.set_psk(None)
    response = client.get("/nonexistent")
    assert response.status_code == 404  # 中间件跳过，路由未匹配


async def test_middleware_concurrent_same_seq_only_one_passes() -> None:
    """并发同 seq 请求恰好放行一个（SC-M1 回归）。

    旧实现的 seq 检查与 ``set_last_seq`` 之间隔着 ``await request.body()``
    挂起点，两个同 seq 并发请求可同时通过检查（重放防护并发失效）；
    修复后「检查-验签-写入」为无 await 原子段，后到者必被 401 拦截。
    """
    state.reset_state()
    state.set_psk(_TEST_PSK)

    # 用带 body 的 POST 确保请求在 body 读取处真实挂起，竞态可被触发
    body_str = '{"k": "v"}'
    canonical = _build_canonical("POST", "/nonexistent", body_str, 1)
    signature = _sign(_TEST_PSK, canonical)
    headers = {"X-Signature": signature, "X-Request-Seq": "1"}

    transport = httpx.ASGITransport(app=app)
    async with httpx.AsyncClient(transport=transport, base_url="http://test") as ac:
        responses = await asyncio.gather(
            ac.post("/nonexistent", content=body_str.encode("utf-8"), headers=headers),
            ac.post("/nonexistent", content=body_str.encode("utf-8"), headers=headers),
        )

    statuses = sorted(r.status_code for r in responses)
    # 恰好一个通过中间件（404：路由未匹配），另一个被 401 拦截
    assert statuses == [401, 404]


def test_middleware_lenient_seq_format_rejected(client: TestClient) -> None:
    """ASCII 内的宽松 seq 格式（"+1"/" 1"）→ 401，不得被 int() 宽松接受（SC-M1）。"""
    for bad_seq in ("+1", " 1"):
        response = client.get(
            "/nonexistent",
            headers={"X-Signature": "0" * 64, "X-Request-Seq": bad_seq},
        )
        assert response.status_code == 401, f"seq={bad_seq!r} 应被拒绝"
        assert "SEC-E-002" in response.json()["detail"]


async def _dispatch_status(raw_headers: list[tuple[bytes, bytes]]) -> int:
    """在 ASGI scope 层直接调用中间件 dispatch，返回响应状态码。

    绕过 httpx 的 ASCII header 限制：真实网络上任意字节都可出现在
    header 值中（Starlette 侧按 latin-1 解码），非 ASCII 攻击面必须覆盖。
    """
    from starlette.requests import Request
    from starlette.responses import PlainTextResponse
    from starlette.responses import Response as StarletteResponse

    from app.middleware.hmac_auth import HMACMiddleware

    async def receive() -> dict[str, object]:
        # GET 无 body：一次性返回空 body + more_body=False 即可
        return {"type": "http.request", "body": b"", "more_body": False}

    scope = {
        "type": "http",
        "asgi": {"version": "3.0"},
        "http_version": "1.1",
        "method": "GET",
        "scheme": "http",
        "path": "/nonexistent",
        "raw_path": b"/nonexistent",
        "query_string": b"",
        "root_path": "",
        "headers": raw_headers,
        "client": ("127.0.0.1", 12345),
        "server": ("test", 80),
    }
    # receive 必须经构造参数传入：Starlette Request 不读 scope["receive"]
    request = Request(scope, receive=receive)

    async def call_next(req: Request) -> StarletteResponse:
        return PlainTextResponse(status_code=404)

    middleware = HMACMiddleware(app=app)
    resp = await middleware.dispatch(request, call_next)
    return resp.status_code


async def test_middleware_non_ascii_seq_rejected() -> None:
    """非 ASCII seq（全角数字）→ 401（SC-M1）。

    httpx 发不出非 ASCII header，用原始 ASGI scope 构造；
    Starlette 按 latin-1 解码后得到非 ASCII str，必须被拒绝。
    """
    state.reset_state()
    state.set_psk(_TEST_PSK)
    status = await _dispatch_status(
        [
            (b"x-signature", b"0" * 64),
            (b"x-request-seq", "１".encode()),
        ]
    )
    assert status == 401


async def test_middleware_non_ascii_signature_returns_401() -> None:
    """非 ASCII 签名头 → 401 验签失败，不得抛 TypeError 变 500（SC-M1）。

    旧实现把非 ASCII str 直接交给 compare_digest 会抛 TypeError；
    httpx 发不出该 header，用原始 ASGI scope 构造覆盖真实攻击面。
    """
    state.reset_state()
    state.set_psk(_TEST_PSK)
    status = await _dispatch_status(
        [
            (b"x-signature", "é".encode() * 64),
            (b"x-request-seq", b"1"),
        ]
    )
    assert status == 401
