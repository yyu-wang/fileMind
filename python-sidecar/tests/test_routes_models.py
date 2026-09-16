"""T2 — ``/models`` 下载路由单元测试。

覆盖：
1. 未签名调用 → 401（证明路由不在 HMAC 豁免名单里）。
2. 已签名 ``GET /models/download/status`` → 200，返回结构化状态（初始 idle）。
3. 未知模型 → 400（ValueError 转 HTTPException）。
4. 已签名 ``POST /models/download`` → 200，状态进入 downloading，且不发起真实
   网络请求（后台任务被替换为 no-op）。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from fastapi.testclient import TestClient

if TYPE_CHECKING:
    from collections.abc import Generator

    from httpx import Response as HttpxResponse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app import state  # noqa: E402
from app.main import app  # noqa: E402
from app.services import model_download_service as svc  # noqa: E402

MODEL = "bge-large-zh-v1.5"
_TEST_PSK: bytes = b"test-psk-32-bytes-need-32-bytes!!"


def _sign(psk: bytes, message: str) -> str:
    """HMAC-SHA256 hex 签名（与 app.middleware.hmac_auth 一致）。"""
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _headers(method: str, path: str, body: str, seq: int) -> dict[str, str]:
    """构造带签名与序号的请求头。"""
    canonical = f"{method}|{path}|{body}|{seq}"
    return {"X-Signature": _sign(_TEST_PSK, canonical), "X-Request-Seq": str(seq)}


@pytest.fixture
def client(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[TestClient, None, None]:
    """重置全局状态 + 固定 PSK + 独立模型目录 + 干净下载状态。"""
    state.reset_state()
    state.set_psk(_TEST_PSK)
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    svc.reset_state()
    yield TestClient(app)
    svc.reset_state()


def test_status_unsigned_rejected(client: TestClient) -> None:
    """未签名 → 401（/models 非豁免路由）。"""
    resp = client.get("/models/download/status", params={"model_name": MODEL})
    assert resp.status_code == 401
    assert "SEC-E-002" in resp.json()["detail"]


def test_status_signed_returns_idle(client: TestClient) -> None:
    """已签名查询（**含查询串**）→ 200 且返回初始 idle 状态。

    签名口径必须与真实调用方（Rust ``proxy::forward_get``）一致：把
    ``path?query`` 整体参与签名。此前这里只签裸 path，恰好绕过了
    「查询串未纳入验签」的缺陷，导致该缺陷在单测里看不见。
    """
    path = f"/models/download/status?model_name={MODEL}"
    resp: HttpxResponse = client.get(path, headers=_headers("GET", path, "", 1))
    assert resp.status_code == 200
    data = resp.json()
    assert data["model_name"] == MODEL
    assert data["status"] == "idle"
    assert data["attempt"] == 0
    assert data["error"] is None


def test_status_unknown_model_400(client: TestClient) -> None:
    """未知模型 → 400（而非 500）。"""
    path = "/models/download/status?model_name=nope"
    resp = client.get(path, headers=_headers("GET", path, "", 1))
    assert resp.status_code == 400
    assert "未知 Embedding 模型" in resp.json()["detail"]


def test_start_download_signed_enters_downloading(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    """已签名启动下载 → 200 且状态为 downloading（后台任务替换为 no-op）。"""

    async def _noop(model: str) -> None:
        return None

    monkeypatch.setattr(svc, "_download_all", _noop)

    path = "/models/download"
    body = json.dumps({"model_name": MODEL})
    resp = client.post(
        path,
        content=body,
        headers={**_headers("POST", path, body, 1), "Content-Type": "application/json"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "downloading"
    assert data["model_name"] == MODEL


def test_start_download_unknown_model_400(client: TestClient) -> None:
    """未知模型启动下载 → 400。"""
    path = "/models/download"
    body = json.dumps({"model_name": "nope"})
    resp = client.post(
        path,
        content=body,
        headers={**_headers("POST", path, body, 1), "Content-Type": "application/json"},
    )
    assert resp.status_code == 400
