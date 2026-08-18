"""文件内容哈希计算服务。

使用 SHA-256（与 Rust 端 ``models.rs`` 注释、``gen_testdata.py`` 口径一致）。
分块读取 8KB chunk，大文件不 OOM。

安全映射：无（纯计算，不涉及 IPC / 存储）。
"""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

# 默认分块大小：8KB（平衡 syscall 次数与内存占用）
_DEFAULT_CHUNK_SIZE = 8192


def compute_content_hash(file_path: Path, chunk_size: int = _DEFAULT_CHUNK_SIZE) -> str:
    """分块读取文件计算 SHA-256 hex 字符串。

    Args:
        file_path: 文件绝对路径
        chunk_size: 每次读取字节数（默认 8KB）

    Returns:
        64 字符的 SHA-256 hex 字符串

    Raises:
        FileNotFoundError: 文件不存在
        OSError: 文件读取失败
    """
    h = hashlib.sha256()
    with open(file_path, "rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()
