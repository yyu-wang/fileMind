"""Rerank 模型的下载规格与就绪判定（复用既有下载管线）。

背景：Rerank 不在 Embedding 注册表内（无 dim / 表名语义），但必须复用
``model_download_service`` 的镜像轮询 / 重试 / 进度管线——否则全新机器备不齐模型
（没有 HF 缓存、hf-mirror 又可能不可达），知识问答会整体不可用。

覆盖：规格解析（两种写法等价）、落盘位置、未知模型报错、就绪判定（空文件不算）、
以及 rerank 加载目标对本地副本的优先选择。下载管线本身由
``test_model_download_service.py`` 覆盖（同一批函数）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import model_download_service as svc  # noqa: E402
from app.services import model_specs, rerank_service  # noqa: E402


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """每个用例独立模型目录 + 干净状态（与 test_model_download_service 同款隔离）。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    monkeypatch.delenv("FILEMIND_RERANK_MODEL", raising=False)
    svc.reset_state()
    yield
    svc.reset_state()


def _fill_local_copy() -> Path:
    """按规格在本地目录写入全部文件（非空），返回落盘根目录。"""
    root = model_specs.model_dir_for(model_specs.RERANK_MODEL_NAME)
    root.mkdir(parents=True, exist_ok=True)
    for name in svc.files_for(model_specs.RERANK_MODEL_NAME):
        (root / name).write_bytes(b"x")
    return root


def test_spec_accepts_dir_name_and_repo() -> None:
    """两种写法（本地目录名 / 仓库 id）解析到同一规格。"""
    by_name = model_specs.resolve_spec(model_specs.RERANK_MODEL_NAME)
    by_repo = model_specs.resolve_spec(model_specs.RERANK_REPO)

    assert by_name == by_repo
    assert by_name.repo == "BAAI/bge-reranker-v2-m3"
    assert by_name.weight == "model.safetensors"
    assert by_name.root.name == model_specs.RERANK_MODEL_NAME
    assert "model.safetensors" in by_name.files


def test_spec_unknown_model_raises() -> None:
    """既非 Rerank 也不在 Embedding 注册表 → ValueError（调用方转 400，不落 5xx）。"""
    with pytest.raises(ValueError):
        model_specs.resolve_spec("no-such-model")


def test_ready_requires_all_files_non_empty() -> None:
    """文件齐备（非空）才算就绪；空文件视为「半截下载」。"""
    model = model_specs.RERANK_MODEL_NAME
    root = _fill_local_copy()
    assert svc.model_ready(model) is True

    (root / "config.json").write_bytes(b"")
    assert svc.model_ready(model) is False


def test_load_target_prefers_local_copy() -> None:
    """本地副本就绪 → rerank 加载目标指向本地目录（离线可用，不依赖 HF 缓存）。"""
    _fill_local_copy()
    expected = str(model_specs.model_dir_for(model_specs.RERANK_MODEL_NAME))
    assert rerank_service.resolve_load_target() == expected
