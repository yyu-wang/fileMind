"""`GET /metrics` 内存监控路由单元测试。

覆盖：
1. **未签名调用（PSK 已 set，非 dev 模式）** → 401 拒绝（证明路由不被加入 HMAC 豁免路径）。
2. **已签名调用** → 200，``rss_mb / vms_mb`` 为正数 float、``threshold_mb=300``、
   ``within_limit`` 为 bool（测试环境必为 True，因为冷启动远低于 300MB）。
3. **错误签名** → 401 签名验证失败（防第三方构造假请求读状态）。
4. **seq 重放** → 401 防重放（证明 /metrics 走完整 HMAC 流程）。
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
    """HMAC-SHA256 hex 签名（与 app.middleware.hmac_auth._sign 一致）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _build_canonical(method: str, path: str, body: str, seq: int) -> str:
    """构造请求 canonical string（与 hmac_auth 一致）。"""
    return f"{method}|{path}|{body}|{seq}"


def _signed_get(
    client: TestClient,
    path: str,
    seq: int,
    *,
    body: str = "",
    psk: bytes = _TEST_PSK,
) -> HttpxResponse:
    """携带正确签名 + seq 调用 GET 路由。"""
    canonical = _build_canonical("GET", path, body, seq)
    signature = _sign(psk, canonical)
    resp: HttpxResponse = client.get(
        path,
        headers={"X-Signature": signature, "X-Request-Seq": str(seq)},
    )
    return resp


@pytest.fixture
def client() -> TestClient:
    """每个测试前重置全局状态并注入固定 PSK（确保 HMAC 中间件不跳过验签）。"""
    state.reset_state()
    state.set_psk(_TEST_PSK)
    return TestClient(app)


def test_metrics_unsigned_rejected(client: TestClient) -> None:
    """未携带签名头 + PSK 已设置（非 dev 模式）→ 401。"""
    resp = client.get("/metrics")
    assert resp.status_code == 401
    assert "SEC-E-002" in resp.json()["detail"]


def test_metrics_signed_success(client: TestClient) -> None:
    """携带正确签名的 GET 请求返回 200 + 四字段符合类型与阈值。"""
    resp = _signed_get(client, "/metrics", seq=1)
    assert resp.status_code == 200
    data = resp.json()
    # 类型校验：rss/vms 为正数 float，threshold 为 300，within_limit 必 True
    assert isinstance(data["rss_mb"], (int, float)), "rss_mb 应为数值"
    assert isinstance(data["vms_mb"], (int, float)), "vms_mb 应为数值"
    assert float(data["rss_mb"]) > 0, "冷启动 RSS 应 > 0"
    assert data["threshold_mb"] == 300
    assert isinstance(data["within_limit"], bool)
    # 冷启动必然低于 300MB；若此处 False 说明测试环境异常（需人工排查）
    assert data["within_limit"], "冷启动内存应低于 300MB"
    # 响应中不得泄漏 PSK
    assert "test-psk-32" not in resp.text.lower()


def test_metrics_wrong_signature_rejected(client: TestClient) -> None:
    """签名错误 → 401。"""
    resp = client.get(
        "/metrics",
        headers={"X-Signature": "0" * 64, "X-Request-Seq": "1"},
    )
    assert resp.status_code == 401
    assert "签名验证失败" in resp.json()["detail"]


def test_metrics_seq_replay_rejected(client: TestClient) -> None:
    """同一 seq 连调两次 → 第二次 401（防重放）。"""
    resp1 = _signed_get(client, "/metrics", seq=42)
    assert resp1.status_code == 200
    resp2 = _signed_get(client, "/metrics", seq=42)
    assert resp2.status_code == 401
    assert "序号重放" in resp2.json()["detail"]
