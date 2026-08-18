"""T2.3 — hash_service 单元测试。"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.hash_service import compute_content_hash  # noqa: E402


def test_same_content_same_hash(tmp_path: Path) -> None:
    """相同内容 → 相同 SHA-256 hex。"""
    f = tmp_path / "a.txt"
    f.write_bytes(b"hello filemind")
    h1 = compute_content_hash(f)
    h2 = compute_content_hash(f)
    assert h1 == h2
    assert len(h1) == 64  # SHA-256 hex = 64 chars


def test_different_content_different_hash(tmp_path: Path) -> None:
    """不同内容 → 不同 hash。"""
    f1 = tmp_path / "a.txt"
    f1.write_bytes(b"hello filemind")
    f2 = tmp_path / "b.txt"
    f2.write_bytes(b"hello filemind!")
    assert compute_content_hash(f1) != compute_content_hash(f2)


def test_matches_stdlib_sha256(tmp_path: Path) -> None:
    """与 hashlib.sha256 一次性计算结果一致。"""
    data = b"x" * 100000  # 100KB
    f = tmp_path / "big.txt"
    f.write_bytes(data)
    expected = hashlib.sha256(data).hexdigest()
    assert compute_content_hash(f) == expected


def test_large_file_no_oom(tmp_path: Path) -> None:
    """10MB 文件不 OOM（分块读取验证）。"""
    data = b"\0" * (10 * 1024 * 1024)
    f = tmp_path / "large.bin"
    f.write_bytes(data)
    h = compute_content_hash(f, chunk_size=8192)
    assert len(h) == 64
    # 与标准库一致
    assert h == hashlib.sha256(data).hexdigest()


def test_nonexistent_file_raises(tmp_path: Path) -> None:
    """文件不存在 → FileNotFoundError。"""
    with pytest.raises(FileNotFoundError):
        compute_content_hash(tmp_path / "nope.txt")
