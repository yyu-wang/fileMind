"""T5.1 — services.embedding_service 单元测试。

覆盖：批量/单条向量生成、空输入、模型未下载、加载失败、推理异常、超时、
返回数量异常、CLS 池化与 L2 归一化、分批、单例复用、模型路径解析、
未注册模型。

全部用例 mock ``_load``（返回 fake 会话/tokenizer）或用临时空目录触发
「未下载」，不加载真实 311MB 权重。pyproject 配 asyncio_mode=auto。
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING
from unittest import mock

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services import embedding_service  # noqa: E402
from app.services.embedding_service import (  # noqa: E402
    EMBEDDING_MODEL,
    EmbeddingUnavailableError,
    embed_text,
    embed_texts,
    model_dir,
    models_root,
    onnx_path,
    reset_model,
)


class _FakeEncoding:
    """模拟 tokenizers.Encoding（只用 ids / attention_mask）。"""

    def __init__(self, ids: list[int], attention: list[int]) -> None:
        self.ids = ids
        self.attention_mask = attention


class _FakeTokenizer:
    """模拟 Tokenizer：记录批大小，返回等长方块。"""

    def __init__(self, seq_len: int = 3) -> None:
        self._seq_len = seq_len
        self.batches: list[int] = []

    def encode_batch(self, texts: list[str]) -> list[_FakeEncoding]:
        self.batches.append(len(texts))
        return [
            _FakeEncoding(ids=list(range(self._seq_len)), attention=[1] * self._seq_len)
            for _ in texts
        ]


class _FakeSession:
    """模拟 onnxruntime.InferenceSession：按输入批大小返回 last_hidden_state。

    ``hidden`` 为三维张量 [batch, seq, dim]；pooling 取 ``[:, 0, :]``。
    """

    def __init__(self, hidden: list[list[list[float]]] | None = None) -> None:
        self._hidden = hidden
        self.runs = 0

    def run(self, output_names: object, feed: dict[str, object]) -> list[object]:
        self.runs += 1
        if self._hidden is not None:
            return [self._hidden]
        # 默认：CLS 向量 = [1, 0]，其余 token 为 [0, 1]（便于断言 CLS 池化）
        batch = len(feed["input_ids"])  # type: ignore[arg-type]
        return [[[[1.0, 0.0], [0.0, 1.0], [0.0, 1.0]] for _ in range(batch)]]


def _fake_load(
    hidden: list[list[list[float]]] | None = None,
) -> tuple[_FakeSession, _FakeTokenizer]:
    """构造 ``_load`` 的返回值（fake 会话 + fake tokenizer）。"""
    return _FakeSession(hidden), _FakeTokenizer()


@pytest.fixture(autouse=True)
def _reset_singleton() -> Generator[None, None, None]:
    """每个用例前后重置模块级单例（避免跨用例复用上例的 fake）。"""
    reset_model()
    yield
    reset_model()


async def test_embed_texts_returns_normalized_cls_vectors() -> None:
    """批量输入 → 等长向量；取 CLS（首 token）并 L2 归一化。"""
    session = _FakeSession()
    tokenizer = _FakeTokenizer()
    with mock.patch.object(embedding_service, "_load", return_value=(session, tokenizer)):
        result = await embed_texts(["a", "b"])
    assert result == [[1.0, 0.0], [1.0, 0.0]]
    assert tokenizer.batches == [2]


async def test_embed_texts_normalizes_non_unit_vectors() -> None:
    """非单位向量被 L2 归一化（保证与查询侧归一化同一尺度）。"""
    hidden = [[[3.0, 4.0], [0.0, 1.0]]]  # CLS = [3,4] → 归一化 [0.6, 0.8]
    with mock.patch.object(embedding_service, "_load", return_value=_fake_load(hidden)):
        result = await embed_texts(["a"])
    assert result[0] == pytest.approx([0.6, 0.8])


async def test_embed_text_single() -> None:
    """单条文本 → 单个向量。"""
    hidden = [[[0.0, 5.0], [1.0, 0.0]]]
    with mock.patch.object(embedding_service, "_load", return_value=_fake_load(hidden)):
        result = await embed_text("hello")
    assert result == pytest.approx([0.0, 1.0])


async def test_embed_texts_empty_input() -> None:
    """空列表 → 空列表，不加载模型。"""
    with mock.patch.object(embedding_service, "_load") as patched:
        result = await embed_texts([])
    assert result == []
    patched.assert_not_called()


async def test_embed_texts_batches_by_encode_batch_size(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """超过 ENCODE_BATCH_SIZE 时按批切分（避免整批 padding 到最长序列）。"""
    monkeypatch.setattr(embedding_service, "ENCODE_BATCH_SIZE", 2)
    tokenizer = _FakeTokenizer()
    with mock.patch.object(embedding_service, "_load", return_value=(_FakeSession(), tokenizer)):
        result = await embed_texts(["a", "b", "c", "d", "e"])
    assert len(result) == 5
    assert tokenizer.batches == [2, 2, 1]


async def test_embed_model_not_downloaded_hints_download(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """模型文件缺失 → EmbeddingUnavailableError 且消息引导去设置页下载。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    with pytest.raises(EmbeddingUnavailableError, match="未下载"):
        await embed_texts(["x"])


async def test_embed_load_failure_wrapped() -> None:
    """加载抛非预期异常 → 统一包装为 EmbeddingUnavailableError。"""
    with (
        mock.patch.object(
            embedding_service,
            "_load",
            side_effect=EmbeddingUnavailableError("Embedding 模型加载失败: boom"),
        ),
        pytest.raises(EmbeddingUnavailableError, match="加载失败"),
    ):
        await embed_texts(["x"])


async def test_embed_encode_error_raises_unavailable() -> None:
    """推理异常 → 统一 EmbeddingUnavailableError（消息含「调用失败」）。"""
    session = mock.MagicMock()
    session.run.side_effect = RuntimeError("ort boom")
    with (
        mock.patch.object(embedding_service, "_load", return_value=(session, _FakeTokenizer())),
        pytest.raises(EmbeddingUnavailableError, match="Embedding 调用失败"),
    ):
        await embed_texts(["x"])


async def test_embed_timeout_raises_unavailable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """超时 → EmbeddingUnavailableError 含超时提示（超时窗口压到 10ms，编码耗 200ms）。"""
    monkeypatch.setattr(embedding_service, "EMBED_TIMEOUT", 0.01)

    def _slow_encode(*args: object, **kwargs: object) -> list[list[float]]:
        time.sleep(0.2)
        return [[1.0]]

    with (
        mock.patch.object(embedding_service, "_load", return_value=_fake_load()),
        mock.patch.object(embedding_service, "_encode", _slow_encode),
        pytest.raises(EmbeddingUnavailableError, match="超时"),
    ):
        await embed_texts(["x"])


async def test_embed_count_mismatch_raises() -> None:
    """返回向量数量与输入不一致 → EmbeddingUnavailableError。"""
    with (
        mock.patch.object(
            embedding_service,
            "_load",
            return_value=(_FakeSession(), _FakeTokenizer()),
        ),
        mock.patch.object(
            embedding_service,
            "_encode_batch",
            return_value=[[1.0]],
        ),
        pytest.raises(EmbeddingUnavailableError, match="数量异常"),
    ):
        await embed_texts(["a", "b"])


async def test_embed_reuses_single_model() -> None:
    """模块级单例：多次调用只加载一次会话（避免重复加载 311MB 权重）。"""
    with mock.patch.object(embedding_service, "_load", return_value=_fake_load()) as patched:
        await embed_texts(["a"])
        await embed_texts(["b"])
    assert patched.call_count == 1


def test_models_root_env_precedence(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """FILEMIND_MODEL_DIR 优先于 FILEMIND_DATA_HOME/models。"""
    monkeypatch.delenv("FILEMIND_MODEL_DIR", raising=False)
    monkeypatch.setenv("FILEMIND_DATA_HOME", str(tmp_path / "data"))
    assert models_root() == tmp_path / "data" / "models"
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path / "custom"))
    assert models_root() == tmp_path / "custom"


def test_model_and_onnx_path_layout(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """本地布局：{models_root}/{model}/{onnx_file}。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    assert model_dir("bge-large-zh-v1.5") == tmp_path / "bge-large-zh-v1.5"
    assert onnx_path("bge-large-zh-v1.5") == (
        tmp_path / "bge-large-zh-v1.5" / "onnx" / "model_quantized.onnx"
    )
    assert EMBEDDING_MODEL == "bge-large-zh-v1.5"


def test_unregistered_model_rejected(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """未注册模型（含 env 指向自定义仓库的场景）→ EmbeddingUnavailableError。"""
    monkeypatch.setenv("FILEMIND_MODEL_DIR", str(tmp_path))
    with pytest.raises(EmbeddingUnavailableError, match="未知 Embedding 模型"):
        model_dir("not-registered")
