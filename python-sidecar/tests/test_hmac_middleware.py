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

import hashlib
import hmac
from typing import TYPE_CHECKING, cast

import pytest
from fastapi.testclient import TestClient

from app import state
from app.main import app

if TYPE_CHECKING:
    from httpx import Response

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


def _signed_get(client: TestClient, path: str, seq: int, psk: bytes = _TEST_PSK) -> Response:
    """携带正确签名 + seq 访问 GET 路由。"""
    canonical = _build_canonical("GET", path, "", seq)
    signature = _sign(psk, canonical)
    return cast(
        "Response",
        client.get(
            path,
            headers={"X-Signature": signature, "X-Request-Seq": str(seq)},
        ),
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


def test_middleware_docs_exempt(client: TestClient) -> None:
    """/docs 在 EXEMPT_PATHS 中 → 无签名头也放行。"""
    response = client.get("/docs")
    assert response.status_code == 200


def test_middleware_openapi_exempt(client: TestClient) -> None:
    """/openapi.json 在 EXEMPT_PATHS 中 → 无签名头也放行。"""
    response = client.get("/openapi.json")
    assert response.status_code == 200


def test_middleware_dev_mode_skips_verify(client: TestClient) -> None:
    """dev 模式（PSK 未设置）所有非豁免路由跳过验签。

    无签名头访问 /nonexistent → 中间件跳过 → 路由返回 404（而非 401），
    证明 PSK=None 时中间件未执行验签。
    """
    state.set_psk(None)
    response = client.get("/nonexistent")
    assert response.status_code == 404  # 中间件跳过，路由未匹配
