"""T6.7 — services.inference_probe_service 单元测试。

覆盖：Ollama 可用（LLM 过滤 + :latest 剥离）、**Embedding 就绪按本地模型文件判定**
（与 Ollama 是否装过同名模型无关）、Ollama 连接失败、响应结构异常。

全部用例 mock httpx（不走真实 Ollama），并把 ``FILEMIND_MODEL_DIR`` 指向用例
独立目录（避免读到开发机上的真实模型文件导致结果不确定）。pyproject 配
asyncio_mode=auto，async 测试函数自动运行。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

import httpx
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.core.embedding_models import MODEL_REGISTRY  # noqa: E402
from app.services import model_download_service  # noqa: E402
from app.services.inference_probe_service import probe_ollama  # noqa: E402

MODEL = "bge-large-zh-v1.5"


class _FakeResponse:
    """模拟 httpx.Response（只带 raise_for_status + json）。"""

    def __init__(self, payload: dict[str, object]) -> None:
        self._payload = payload

    def raise_for_status(self) -> None:
        return None

    def json(self) -> dict[str, object]:
        return self._payload


def _fake_client(payload: dict[str, object]) -> mock.AsyncMock:
    """构造返回固定 /api/tags payload 的 fake AsyncClient（async 上下文）。"""
    client = mock.AsyncMock()
    client.get.return_value = _FakeResponse(payload)
    client.__aenter__.return_value = client
    client.__aexit__.return_value = False
    return client


TAGS_PAYLOAD: dict[str, object] = {
    "models": [
        {
            "name": "qwen3.8-27b",
            "size": 16_000_000_000,
            "modified_at": "2026-08-01T00:00:00Z",
            "details": {"family": "qwen3"},
        },
        {
            "name": "qllama/bge-large-zh-v1.5",
            "size": 1_300_000_000,
            "modified_at": "2026-08-02T00:00:00Z",
            "details": {"family": "bert"},
        },
    ]
}


@pytest.fixture(autouse=True)
def model_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """把模型根目录指向用例独立目录，并返回该路径（供就绪用例写入文件）。"""
    root = tmp_path / "models"
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(root))
    return root


def _write_model_files(model_root: Path) -> None:
    """写入全部模型文件（大小非空），使其判定为「已就绪」。"""
    for name in model_download_service.files_for(MODEL):
        target = model_root / MODEL / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(b"data")


async def test_probe_success_filters_llm_and_reports_model_readiness(
    model_root: Path,
) -> None:
    """可用：LLM 排除 embedding 注册表名；Embedding 可用性 = 本地模型文件是否就绪。

    tags 里有 qllama/bge-large-zh-v1.5，但本地模型目录为空 → 仍为 False
    （Embedding 不再由 Ollama 提供）。
    """
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client(TAGS_PAYLOAD),
    ):
        result = await probe_ollama()

    assert result.available is True
    assert result.status == "ok"
    assert result.error_code is None

    # LLM 列表只含生成模型，embedding 注册表名与 Ollama 残留名被排除
    assert [m.name for m in result.llm_models] == ["qwen3.8-27b"]
    assert result.llm_models[0].size_bytes == 16_000_000_000
    assert result.llm_models[0].family == "qwen3"

    # Embedding：维度/版本透传；本地无模型文件 → 未就绪
    by_name = {m.name: m for m in result.embedding_models}
    assert list(by_name) == [MODEL]
    assert by_name[MODEL].available is False
    assert by_name[MODEL].dim == 1024
    assert by_name[MODEL].version == 1


async def test_probe_reports_ready_when_local_model_files_present(model_root: Path) -> None:
    """本地模型文件齐备 → Embedding available=True（与 Ollama 无关）。"""
    _write_model_files(model_root)

    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client(TAGS_PAYLOAD),
    ):
        result = await probe_ollama()

    by_name = {m.name: m for m in result.embedding_models}
    assert by_name[MODEL].available is True


async def test_probe_ollama_down_still_reports_embedding_readiness(model_root: Path) -> None:
    """Ollama 不可用时，Embedding 就绪状态仍独立按本地文件判定。"""
    _write_model_files(model_root)
    client = mock.AsyncMock()
    client.get.side_effect = httpx.ConnectError("connection refused")
    client.__aenter__.return_value = client
    client.__aexit__.return_value = False
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=client,
    ):
        result = await probe_ollama()

    assert result.available is False
    assert result.status == "unavailable"
    assert result.error_code == "OLLAMA_UNAVAILABLE"
    assert result.llm_models == []
    by_name = {m.name: m for m in result.embedding_models}
    assert by_name[MODEL].available is True  # 文件齐备，与 Ollama 状态无关


async def test_probe_missing_details_field() -> None:
    """details 字段缺失 → family=None，不抛错。"""
    payload: dict[str, object] = {"models": [{"name": "qwen3.8-27b", "size": 100}]}
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client(payload),
    ):
        result = await probe_ollama()

    assert result.available is True
    assert len(result.llm_models) == 1
    assert result.llm_models[0].family is None
    assert result.llm_models[0].modified_at is None


async def test_probe_malformed_response_returns_unavailable() -> None:
    """/api/tags 响应结构异常 → ValidationError → available=false，不 5xx。"""
    with mock.patch(
        "app.services.inference_probe_service.httpx.AsyncClient",
        return_value=_fake_client({"models": "boom"}),
    ):
        result = await probe_ollama()

    assert result.available is False
    assert result.status == "unavailable"
    assert result.error_code == "OLLAMA_UNAVAILABLE"
    assert len(result.embedding_models) == len(MODEL_REGISTRY)
