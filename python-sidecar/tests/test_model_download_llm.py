"""本地生成模型（GGUF）的下载规格与就绪判定（复用既有下载管线）。

背景：内置 llama.cpp 引擎（T3）需要 GGUF 权重，而 GGUF 与 Rerank 同理不在 Embedding
注册表内（无 dim / 表名语义），必须复用 ``model_download_service`` 的镜像轮询 / 重试 /
进度管线——否则未安装 Ollama 的机器仍然拿不到本地生成模型，「无 Ollama 也能知识问答」
就缺最后一块。

覆盖：规格解析（两种写法等价）、落盘位置、单文件权重、未知模型报错、就绪判定
（空文件不算）、以及离线导入清单包含 GGUF（内网分发同一条离线通道）。
下载管线本身由 ``test_model_download_service.py`` 覆盖（同一批函数）。
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
from app.services import model_specs  # noqa: E402


@pytest.fixture(autouse=True)
def _isolate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """每个用例独立模型目录 + 干净状态（与 test_model_download_rerank 同款隔离）。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    svc.reset_state()
    yield
    svc.reset_state()


def _fill_local_copy() -> Path:
    """按规格在本地目录写入全部文件（非空），返回落盘根目录。"""
    root = model_specs.model_dir_for(model_specs.LLM_MODEL_NAME)
    root.mkdir(parents=True, exist_ok=True)
    for name in svc.files_for(model_specs.LLM_MODEL_NAME):
        (root / name).write_bytes(b"x")
    return root


def test_spec_accepts_dir_name_and_repo() -> None:
    """两种写法（本地目录名 / 仓库 id）解析到同一规格。"""
    by_name = model_specs.resolve_spec(model_specs.LLM_MODEL_NAME)
    by_repo = model_specs.resolve_spec(model_specs.LLM_REPO)

    assert by_name == by_repo
    assert by_name.repo == "Qwen/Qwen2.5-3B-Instruct-GGUF"
    assert by_name.weight == "qwen2.5-3b-instruct-q4_k_m.gguf"
    assert by_name.root.name == model_specs.LLM_MODEL_NAME
    # 单文件权重：GGUF 自带词表与超参，没有 tokenizer/config 辅助文件
    assert by_name.files == (model_specs.LLM_WEIGHT_FILE,)


def test_ready_requires_weight_non_empty() -> None:
    """权重齐备（非空）才算就绪；空文件视为「半截下载」。"""
    root = _fill_local_copy()
    assert svc.model_ready(model_specs.LLM_MODEL_NAME) is True

    (root / model_specs.LLM_WEIGHT_FILE).write_bytes(b"")
    assert svc.model_ready(model_specs.LLM_MODEL_NAME) is False


def test_ready_false_before_download() -> None:
    """未下载 → 不就绪（T3 据此决定是否回落到 Ollama）。"""
    assert svc.model_ready(model_specs.LLM_MODEL_NAME) is False


def test_unknown_model_still_raises() -> None:
    """新增一类来源不影响未知模型的报错语义（调用方转 400，不落 5xx）。"""
    with pytest.raises(ValueError):
        model_specs.resolve_spec("no-such-model")


def test_importable_models_includes_gguf() -> None:
    """GGUF 纳入离线导入清单：内网机器同样能用 zip / 目录分发 GGUF。"""
    assert model_specs.LLM_MODEL_NAME in model_specs.importable_models()
