"""T4.5 — 评估数据集生成与加载单元测试。

覆盖：确定性生成、分类分布、边界用例注入、「其他」扩展名约束、
深层路径、JSONL 读写往返、人类可读大小解析。
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from eval.dataset import (  # noqa: E402
    _OTHER_EXTENSIONS,
    CATEGORY_NAMES,
    EDGE_CASES,
    generate_dataset,
    load_jsonl,
    parse_size,
    write_jsonl,
)


def test_generate_deterministic() -> None:
    """相同 seed 生成完全一致，数量正确。"""
    first = generate_dataset(500, seed=42)
    second = generate_dataset(500, seed=42)
    assert first == second
    assert len(first) == 500


def test_category_distribution() -> None:
    """8 个评估分类均有充足样本（边界用例注入后仍 >= 40）。"""
    records = generate_dataset(500, seed=42)
    counts = {category: 0 for category in CATEGORY_NAMES}
    for record in records:
        counts[record.correct_category] += 1
    for category in CATEGORY_NAMES:
        assert counts[category] >= 40, f"{category} 样本过少: {counts[category]}"


def test_other_only_uses_custom_extensions() -> None:
    """「其他」分类只使用自定义扩展名池，避免误命中表格规则。"""
    for record in generate_dataset(500, seed=42):
        if record.correct_category == "其他":
            assert record.file_type in _OTHER_EXTENSIONS


def test_edge_cases_present() -> None:
    """全部边界用例都注入到样本集。"""
    names = {record.file_name for record in generate_dataset(500, seed=42)}
    for name, _, _ in EDGE_CASES:
        assert name in names


def test_nested_path_depth() -> None:
    """路径含分类目录 + 年份 + 季度，层数足够深（>= 5 层）。"""
    for record in generate_dataset(50, seed=42):
        assert len(Path(record.path).parts) >= 5


def test_jsonl_roundtrip() -> None:
    """write_jsonl → load_jsonl 后样本完全一致。"""
    records = generate_dataset(30, seed=42)
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "eval.jsonl"
        write_jsonl(path, records)
        loaded = load_jsonl(path)
    assert loaded == records


def test_parse_size() -> None:
    """人类可读大小 → 字节数。"""
    assert parse_size("0 B") == 0
    assert parse_size("500 B") == 500
    assert parse_size("2.3 KB") == round(2.3 * 1024)
    assert parse_size("2.3 MB") == round(2.3 * 1024 * 1024)
    assert parse_size("2.2 GB") == round(2.2 * 1024**3)
    assert parse_size("乱码") == 0
    assert parse_size("2.3") == 0
