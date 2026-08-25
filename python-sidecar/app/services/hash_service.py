"""文件内容哈希计算服务（已废弃）。

SC-m25：``compute_content_hash`` 已删除——内容哈希由 Rust 层计算
（content_hash + embedding_version 双重判断），Sidecar 不参与。
文件保留防 import 链断裂。
"""

from __future__ import annotations
