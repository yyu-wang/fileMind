"""T2.6 — core.embedding_models 单元测试。

覆盖：list_model_names 有序稳定；get_model_info/get_model_dim 合法/非法分支；
注册表三个模型 dim/default_version 与 MAIN DEFAULT_MODEL 对齐。
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.core import embedding_models  # noqa: E402


def test_list_model_names_sorted_stable() -> None:
    """返回有序 list：字母序升序，3 个注册表项目已就位。"""
    names = embedding_models.list_model_names()
    assert names == sorted(names), "必须返回按名字排序后的稳定列表"
    assert "bge-large-zh-v1.5" in names
    assert "bge-m3" in names
    assert "bge-small-zh-v1.5" in names


def test_default_model_in_registry() -> None:
    """DEFAULT_MODEL 必须是注册表里的一员（启动 fallback 依赖这一点）。"""
    assert embedding_models.DEFAULT_MODEL in embedding_models.MODEL_REGISTRY


def test_each_model_dim_and_default_version_positive() -> None:
    """每个注册的模型：dim >= 128（小模型最小 512）、default_version == 1。"""
    for name, info in embedding_models.MODEL_REGISTRY.items():
        assert info.name == name, f"{name}: info.name 不与 key 对齐"
        assert info.dim >= 128, f"{name}: dim 过小（当前={info.dim}）"
        assert info.dim in {512, 1024}, f"{name}: dim 应是 512 或 1024（约定）"
        assert info.default_version >= 1
        assert info.description, f"{name}: description 不能为空字符串"


def test_get_model_dim_known() -> None:
    """get_model_dim 对已知模型返回正确 dim。"""
    # bge-large-zh-v1.5 = 1024
    assert embedding_models.get_model_dim("bge-large-zh-v1.5") == 1024
    # bge-m3 = 1024
    assert embedding_models.get_model_dim("bge-m3") == 1024
    # bge-small-zh-v1.5 = 512
    assert embedding_models.get_model_dim("bge-small-zh-v1.5") == 512


def test_get_model_info_known_has_name() -> None:
    """get_model_info 返回的 info.name == 入参（key 一致性）。"""
    for n in embedding_models.list_model_names():
        info = embedding_models.get_model_info(n)
        assert info.name == n


def test_get_model_dim_unknown_raises_valueerror() -> None:
    """未知模型抛 ValueError，消息里含可用模型列表（便于排查）。"""
    with pytest.raises(ValueError) as exc_info:
        embedding_models.get_model_dim("fake-model-123")
    msg = exc_info.value.args[0]
    assert "fake-model-123" in msg
    assert "bge-large-zh-v1.5" in msg  # 给出可用列表


def test_get_model_info_unknown_raises_valueerror() -> None:
    """get_model_info 也走同一异常路径。"""
    with pytest.raises(ValueError):
        embedding_models.get_model_info("not-registered")


def test_rates_constants_reasonable() -> None:
    """EST_FILES_PER_MINUTE 与 EST_MINUTES_CAP 应该是合理正整数（自检常量）。"""
    assert 50 <= embedding_models.EST_FILES_PER_MINUTE <= 2000, "粗估速度应该在合理区间"
    assert 60 <= embedding_models.EST_MINUTES_CAP <= 9999, "上限应有上限但不能太低"
