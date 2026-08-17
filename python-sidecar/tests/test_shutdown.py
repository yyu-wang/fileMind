"""`POST /shutdown` 路由单元测试。

覆盖：
1. **未签名调用（PSK 已 set，非 dev 模式）** → 401 拒绝（证明路由不被加入 HMAC 豁免路径，符合安全要求）。
2. **已签名调用** → 200，body 为 ``{"status":"shutting_down"}``。
3. **只接受 POST** → GET/PUT/DELETE 返回 405，避免误触发。

Notes:
    测试通过 `TestClient` 跑，不会启动真实 `uvicorn.Server`；
    `_graceful_exit()` 内部对 server.shutdown() 的调用在 TestClient 场景下
    会提前 return（因为全局 server_shutdown_fn 仅在 lifespan 里 set），
    所以本测试不会真正中断 Python 进程。
"""

from __future__ import annotations

import hashlib
import hmac
from typing import TYPE_CHECKING

import pytest
from fastapi.testclient import TestClient

from app import state
from app.main import app

if TYPE_CHECKING:
    from httpx import Response as HttpxResponse

_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"


def _sign(psk: bytes, message: str) -> str:
    """HMAC-SHA256 hex 签名（与 app.middleware.hmac_auth._sign 算法一致）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _build_canonical(method: str, path: str, body: str, seq: int) -> str:
    """构造请求 canonical string（与 hmac_auth._build_request_canonical 一致）。"""
    return f"{method}|{path}|{body}|{seq}"


@pytest.fixture
def client() -> TestClient:
    """每个测试前重置全局状态并注入固定 PSK（确保 HMAC 中间件不跳过验签）。"""
    state.reset_state()
    state.set_psk(_TEST_PSK)
    return TestClient(app)


def _signed_post(
    client: TestClient,
    path: str,
    seq: int,
    body: str = "",
    *,
    psk: bytes = _TEST_PSK,
) -> HttpxResponse:
    """携带正确签名 + seq 调用 POST 路由。"""
    canonical = _build_canonical("POST", path, body, seq)
    signature = _sign(psk, canonical)
    resp: HttpxResponse = client.post(
        path,
        content=body,
        headers={"X-Signature": signature, "X-Request-Seq": str(seq)},
    )
    return resp


def test_shutdown_unsigned_rejected(client: TestClient) -> None:
    """未携带签名头 + PSK 已设置（非 dev 模式）→ 401。

    安全动机：若 /shutdown 误被加入 HMAC 豁免列表，任何第三方都能让
    Sidecar 退出，导致推理/索引功能不可用。
    """
    resp = client.post("/shutdown")
    assert resp.status_code == 401
    data = resp.json()
    assert "SEC-E-002" in data["detail"]


def test_shutdown_signed_returns_shutting_down(client: TestClient) -> None:
    """携带正确签名的 POST 请求返回 200 shutting_down，响应不泄漏 PSK。"""
    resp = _signed_post(client, "/shutdown", seq=1)
    assert resp.status_code == 200
    data = resp.json()
    assert data == {"status": "shutting_down"}
    # 响应中不应包含 PSK 片段或 set 字面量
    assert "test-psk" not in resp.text.lower()


def test_shutdown_rejects_seq_replay(client: TestClient) -> None:
    """同一 seq 发两次 → 第二次 401 防重放（证明 /shutdown 也走 HMAC 全流程）。"""
    resp1 = _signed_post(client, "/shutdown", seq=10)
    assert resp1.status_code == 200
    resp2 = _signed_post(client, "/shutdown", seq=10)
    assert resp2.status_code == 401
    assert "序号重放" in resp2.json()["detail"]


def test_shutdown_wrong_signature_rejected(client: TestClient) -> None:
    """签名错误 → 401，不能误关 Sidecar。"""
    resp = client.post(
        "/shutdown",
        headers={
            "X-Signature": "0" * 64,
            "X-Request-Seq": "1",
        },
    )
    assert resp.status_code == 401
    assert "签名验证失败" in resp.json()["detail"]


def test_shutdown_method_not_allowed(client: TestClient) -> None:
    """非 POST 方法不会命中 shutdown 逻辑 → 405 或 401 均可。

    在 Starlette 里，中间件（HMAC 验签）**先于**路由匹配执行，所以对非 POST：
    - 若签名缺失 → 中间件先 401（常见）
    - 若签名合法 → 路由匹配阶段 405

    两者都不是 200 shutting_down，安全上等价：都不会真正执行关闭。
    """
    for method in ("GET", "PUT", "DELETE", "PATCH", "OPTIONS"):
        canonical = _build_canonical(method, "/shutdown", "", seq=42)
        signature = _sign(_TEST_PSK, canonical)
        resp = client.request(
            method,
            "/shutdown",
            headers={"X-Signature": signature, "X-Request-Seq": "42"},
        )
        assert resp.status_code in {401, 405}, (
            f"{method} /shutdown 应返回 401/405，但实际 {resp.status_code}"
        )
