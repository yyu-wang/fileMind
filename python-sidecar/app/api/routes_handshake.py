"""Sidecar 握手路由：验证 Rust 客户端身份并返回 proof。

安全映射：S-01（Sidecar 端口冒充）。

握手流程：
1. Rust 客户端生成 nonce（32 字节随机 hex）
2. Rust 用 PSK 对 ``handshake|{nonce}`` 签名，POST 到 ``/handshake``
3. Sidecar 验签 → 添加 nonce 到已用集合（防握手重放）
4. Sidecar 用 PSK 对 ``handshake-ok|{nonce}`` 签名作为 proof 返回
5. Rust 验证 proof → 握手成功，进入正常请求阶段
"""

import hashlib
import hmac

from fastapi import APIRouter, Header, HTTPException

from app import state
from app.models import HandshakeChallenge, HandshakeResponse

router = APIRouter(prefix="/handshake", tags=["握手"])


def _sign(psk: bytes, message: str) -> str:
    """计算 HMAC-SHA256 签名（hex 编码）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _build_handshake_canonical(nonce_hex: str) -> str:
    """构造握手请求 canonical string：``handshake|{nonce_hex}``。"""
    return f"handshake|{nonce_hex}"


def _build_handshake_proof_canonical(nonce_hex: str) -> str:
    """构造握手响应 proof canonical string：``handshake-ok|{nonce_hex}``。"""
    return f"handshake-ok|{nonce_hex}"


@router.post("", response_model=HandshakeResponse)
async def handshake(
    challenge: HandshakeChallenge,
    x_signature: str = Header(..., alias="X-Signature"),
) -> HandshakeResponse:
    """接收 nonce + 验签 + 返回 proof。

    Args:
        challenge: 握手请求体，包含 nonce（hex）。
        x_signature: 客户端对 ``handshake|{nonce}`` 的 HMAC-SHA256 hex 签名。

    Returns:
        包含 ``proof`` 字段的响应，proof 是 Sidecar 对
        ``handshake-ok|{nonce}`` 的 HMAC-SHA256 hex 签名。

    Raises:
        HTTPException 500: PSK 未初始化（dev 模式直连 Sidecar）。
        HTTPException 401: 客户端签名验证失败或 nonce 已使用（重放）。
    """
    psk = state.get_psk()
    if psk is None:
        # dev 模式启动但被请求握手：禁止，避免误用未握手 Sidecar
        raise HTTPException(
            status_code=500,
            detail="SEC-E-001:PSK 未初始化（dev 模式不能被外部握手）",
        )

    # 验证客户端签名
    canonical = _build_handshake_canonical(challenge.nonce)
    expected_signature = _sign(psk, canonical)
    if not hmac.compare_digest(expected_signature, x_signature):
        raise HTTPException(status_code=401, detail="SEC-E-001:握手签名验证失败")

    # nonce 防重放：每个 nonce 只能用一次
    if not state.add_nonce(challenge.nonce):
        raise HTTPException(status_code=401, detail="SEC-E-001:nonce 已使用（重放）")

    # 构造 proof：用 PSK 对 "handshake-ok|{nonce}" 签名
    proof_canonical = _build_handshake_proof_canonical(challenge.nonce)
    proof = _sign(psk, proof_canonical)

    return HandshakeResponse(proof=proof)
