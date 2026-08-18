"""Sidecar 全局状态：PSK、序号与已用 nonce 集合。

类型化的模块级状态，避免直接操作 Starlette ``State``（其 attribute 是动态的，
mypy strict 无法检查）。lifespan 启动时填充 PSK，中间件与路由通过本模块访问。

安全映射：S-01（Sidecar 端口冒充）、T-01（Sidecar 通信篡改）。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.db.lancedb_repo import LanceDBManager

# 握手后的 PSK：生产模式由 stdin 注入，dev 模式为 None（跳过验签）
_psk: bytes | None = None
# 上一个请求的序号（防重放：新请求 seq 必须 > last_seq）
_last_seq: int = 0
# 已使用的握手 nonce 集合（防握手重放）
_used_nonces: set[str] = set()
# LanceDB 管理器（lifespan 初始化后非 None）
_lancedb: LanceDBManager | None = None


def get_psk() -> bytes | None:
    """返回当前 PSK，dev 模式下为 ``None``。"""
    return _psk


def set_psk(psk: bytes | None) -> None:
    """设置 PSK（仅 lifespan 启动时调用一次）。"""
    global _psk
    _psk = psk


def next_seq() -> int:
    """返回递增的下一个请求序号（用于客户端发送）。"""
    global _last_seq
    _last_seq += 1
    return _last_seq


def get_last_seq() -> int:
    """返回当前已见序号（中间件校验 seq 严格递增用）。"""
    return _last_seq


def set_last_seq(seq: int) -> None:
    """更新 last_seq（中间件见到合法 seq 后调用）。"""
    global _last_seq
    _last_seq = seq


def add_nonce(nonce: str) -> bool:
    """添加 nonce 到已用集合。

    Returns:
        ``True`` 表示 nonce 新增成功；``False`` 表示已存在（拒绝重放）。
    """
    if nonce in _used_nonces:
        return False
    _used_nonces.add(nonce)
    return True


def get_lancedb() -> LanceDBManager | None:
    """返回 LanceDB 管理器（未初始化时为 None）。"""
    return _lancedb


def set_lancedb(mgr: LanceDBManager | None) -> None:
    """设置 LanceDB 管理器（lifespan 启动时调用一次；测试关闭/重启时允许 None）。"""
    global _lancedb
    _lancedb = mgr


def reset_state() -> None:
    """重置全部状态（仅测试用，生产代码禁止调用）。"""
    global _psk, _last_seq, _used_nonces, _lancedb
    _psk = None
    _last_seq = 0
    _used_nonces = set()
    _lancedb = None
