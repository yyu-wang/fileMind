"""T2 — ``/models`` 下载路由 + T1 离线导入路由单元测试。

覆盖：
1. 未签名调用 → 401（证明路由不在 HMAC 豁免名单里）。
2. 已签名 ``GET /models/download/status`` → 200，返回结构化状态（初始 idle）。
3. 未知模型 → 400（ValueError 转 HTTPException）。
4. 已签名 ``POST /models/download`` → 200，状态进入 downloading，且不发起真实
   网络请求（后台任务被替换为 no-op）。
5. 已签名 ``POST /models/import`` → 200 透传明细；包不合法 → 422（EMB-V-001）、
   包不可读 → 500（EMB-U-002）——错误码与 ``rules/error-handling.md`` 对齐。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import sys
import zipfile
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
from app.services import model_import_service as import_svc  # noqa: E402
from app.services.model_download_service import model_ready  # noqa: E402
from app.services.model_specs import LLM_MODEL_NAME, resolve_spec  # noqa: E402

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


def test_gguf_model_uses_same_download_route(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    """GGUF（本地生成模型）复用同一条下载链路：同一端点、同一进度口径。

    T2 的关键点即「不加新端点、不加新下载器」——规格注册后端点自动支持。
    """

    async def _noop(model: str) -> None:
        return None

    monkeypatch.setattr(svc, "_download_all", _noop)

    path = "/models/download"
    body = json.dumps({"model_name": LLM_MODEL_NAME})
    resp = client.post(
        path,
        content=body,
        headers={**_headers("POST", path, body, 1), "Content-Type": "application/json"},
    )
    assert resp.status_code == 200
    data = resp.json()
    assert data["model_name"] == LLM_MODEL_NAME
    assert data["status"] == "downloading"


# ------------------------------------------------------------------
# T1 离线模型包导入
# ------------------------------------------------------------------

_RERANK = "bge-reranker-v2-m3"


def _post_import(client: TestClient, source: str, seq: int = 1) -> HttpxResponse:
    """携带 HMAC 签名 POST /models/import。"""
    path = "/models/import"
    body = json.dumps({"path": source})
    resp: HttpxResponse = client.post(
        path,
        content=body,
        headers={**_headers("POST", path, body, seq), "Content-Type": "application/json"},
    )
    return resp


def test_import_signed_passes_through_details(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    """已签名导入 → 200，导入与跳过明细原样透传给设置页。"""

    async def _fake(source: str) -> import_svc.ImportOutcome:
        return import_svc.ImportOutcome(imported=[MODEL], skipped=[_RERANK])

    monkeypatch.setattr(import_svc, "import_package", _fake)

    resp = _post_import(client, "/tmp/offline.zip")
    assert resp.status_code == 200
    assert resp.json() == {"imported": [MODEL], "skipped": [_RERANK]}


def test_import_unsigned_rejected(client: TestClient) -> None:
    """未签名导入 → 401（/models/import 非豁免路由）。"""
    resp = client.post("/models/import", json={"path": "/tmp/offline.zip"})
    assert resp.status_code == 401


def test_import_invalid_package_returns_422(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    """包不合法 → 422 + EMB-V-001，并带上缺失文件明细（用户可据此补包）。"""

    async def _boom(source: str) -> import_svc.ImportOutcome:
        raise import_svc.PackageInvalidError(
            "包内模型目录不完整（bge-large-zh-v1.5 缺少 tokenizer.json）"
        )

    monkeypatch.setattr(import_svc, "import_package", _boom)

    resp = _post_import(client, "/tmp/bad.zip")
    assert resp.status_code == 422
    detail = resp.json()["detail"]
    assert detail.startswith("EMB-V-001:")
    assert "tokenizer.json" in detail


def test_import_unreadable_package_returns_500(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    """包不可读 → 500 + EMB-U-002（非预期错误走通用提示 + 日志）。"""

    async def _boom(source: str) -> import_svc.ImportOutcome:
        raise import_svc.PackageSourceError("zip 无法读取: BadZipFile")

    monkeypatch.setattr(import_svc, "import_package", _boom)

    resp = _post_import(client, "/tmp/broken.zip")
    assert resp.status_code == 500
    assert resp.json()["detail"].startswith("EMB-U-002:")


def test_import_real_package_end_to_end(client: TestClient, tmp_path: Path) -> None:
    """真实包走完整 HTTP 链路：zip → 落盘 → 模型就绪（DoD：内网导入即可用）。

    与上面的用例不同，这里不 mock 服务层：验证请求体字段名（Rust 发 ``{"path": ...}``）、
    包解析、落盘位置三者的真实配合。
    """
    staging = tmp_path / "staging" / "models"
    for model in (MODEL, _RERANK):
        for name in resolve_spec(model).files:
            target = staging / model / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"\x00")
    zip_path = tmp_path / "offline.zip"
    with zipfile.ZipFile(zip_path, "w") as archive:
        for path in staging.rglob("*"):
            if path.is_file():
                archive.write(path, path.relative_to(staging).as_posix())

    resp = _post_import(client, str(zip_path))

    assert resp.status_code == 200
    assert sorted(resp.json()["imported"]) == sorted([MODEL, _RERANK])
    assert model_ready(MODEL)
    assert model_ready(_RERANK)
