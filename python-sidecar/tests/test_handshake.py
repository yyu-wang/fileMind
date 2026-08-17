"""握手路由单元测试：握手成功/失败/nonce 重放/dev 模式隔离。

安全映射：S-01（Sidecar 端口冒充）。

测试覆盖：
- 正确 PSK + 正确签名 → 200 并返回可验证的 proof
- 错误签名 → 401
- 相同 nonce 二次握手 → 401（防握手重放）
- PSK 未初始化（dev 模式被外部握手）→ 500
- /handshake 在 EXEMPT_PATHS 中，不被 HMAC 中间件拦截
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

# 固定测试 PSK（32 字节，贴近真实 PSK_LEN；HMAC-SHA256 任意长度 key 均可）
_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"
# 测试用 nonce（32 字节随机数的 hex 编码，64 字符）
_TEST_NONCE: str = "aa" * 32


def _sign(psk: bytes, message: str) -> str:
    """构造 HMAC-SHA256 hex 签名。

    与 ``app.api.routes_handshake._sign`` 算法一致，复制而非 import 私有函数，
    保证测试独立性的同时维持算法等价（HMAC-SHA256 为标准算法，不会漂移）。
    """
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


@pytest.fixture
def client() -> TestClient:
    """每个测试前重置全局状态并注入固定 PSK。

    不进入 ``TestClient`` 的 with 上下文，避免 lifespan 触发 stdin 读取
    覆盖 fixture 已设置的 PSK。
    """
    state.reset_state()
    state.set_psk(_TEST_PSK)
    return TestClient(app)


def _post_handshake(client: TestClient, nonce: str, signature: str) -> Response:
    """POST /handshake 携带 nonce 与 X-Signature 头。"""
    return cast(
        "Response",
        client.post(
            "/handshake",
            json={"nonce": nonce},
            headers={"X-Signature": signature},
        ),
    )


def test_handshake_success(client: TestClient) -> None:
    """正确 PSK + 正确签名 → 200 并返回可被 PSK 验证的 proof。"""
    canonical = f"handshake|{_TEST_NONCE}"
    signature = _sign(_TEST_PSK, canonical)
    response = _post_handshake(client, _TEST_NONCE, signature)

    assert response.status_code == 200
    data = response.json()
    assert "proof" in data

    # 验证 proof 是 Sidecar 用 PSK 对 "handshake-ok|{nonce}" 的签名（双向验证）
    proof_canonical = f"handshake-ok|{_TEST_NONCE}"
    expected_proof = _sign(_TEST_PSK, proof_canonical)
    assert data["proof"] == expected_proof


def test_handshake_wrong_signature(client: TestClient) -> None:
    """错误签名（全 0）→ 401。"""
    wrong_sig = "0" * 64
    response = _post_handshake(client, _TEST_NONCE, wrong_sig)

    assert response.status_code == 401
    assert "SEC-E-001" in response.json()["detail"]


def test_handshake_nonce_replay(client: TestClient) -> None:
    """相同 nonce 二次握手 → 第二次 401（防握手重放）。"""
    canonical = f"handshake|{_TEST_NONCE}"
    signature = _sign(_TEST_PSK, canonical)

    # 第一次握手成功
    first = _post_handshake(client, _TEST_NONCE, signature)
    assert first.status_code == 200

    # 第二次重放相同 nonce → 401
    second = _post_handshake(client, _TEST_NONCE, signature)
    assert second.status_code == 401
    assert "nonce" in second.json()["detail"]


def test_handshake_dev_mode_no_psk(client: TestClient) -> None:
    """PSK 未初始化（dev 模式被外部握手）→ 500，禁止误用未握手 Sidecar。"""
    state.set_psk(None)  # 模拟 dev 模式
    signature = _sign(_TEST_PSK, f"handshake|{_TEST_NONCE}")
    response = _post_handshake(client, _TEST_NONCE, signature)

    assert response.status_code == 500
    assert "SEC-E-001" in response.json()["detail"]


def test_handshake_exempt_from_hmac_middleware(client: TestClient) -> None:
    """/handshake 在 EXEMPT_PATHS 中，HMAC 中间件对其放行。

    证明方式：握手请求只带 X-Signature（握手头），不带中间件要求的 X-Request-Seq，
    仍能成功返回 200 —— 说明中间件未对 /handshake 执行签名+seq 校验。
    """
    canonical = f"handshake|{_TEST_NONCE}"
    signature = _sign(_TEST_PSK, canonical)
    response = client.post(
        "/handshake",
        json={"nonce": _TEST_NONCE},
        headers={"X-Signature": signature},  # 故意不带 X-Request-Seq
    )
    assert response.status_code == 200
