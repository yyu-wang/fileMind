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
# SC-m5：加上限防长期运行无界增长；超限时清空（旧 nonce 已过 TTL 无意义）
_used_nonces: set[str] = set()
MAX_NONCES = 10000
# LanceDB 管理器（lifespan 初始化后非 None）
_lancedb: LanceDBManager | None = None
# 当前 Embedding 模型名（lifespan 启动时注入 DEFAULT_MODEL）
_current_model: str | None = None
# 当前 Embedding 模型版本号（与 LanceDB documents_{model}_v{N} 对应）
_current_embedding_version: int | None = None


def get_psk() -> bytes | None:
    """返回当前 PSK，dev 模式下为 ``None``。"""
    return _psk


def set_psk(psk: bytes | None) -> None:
    """设置 PSK（仅 lifespan 启动时调用一次）。"""
    global _psk
    _psk = psk


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
    # SC-m5：超上限时清空（nonce 是握手防重放，旧 nonce 已过 TTL 无意义）
    if len(_used_nonces) >= MAX_NONCES:
        _used_nonces.clear()
    _used_nonces.add(nonce)
    return True


def get_lancedb() -> LanceDBManager | None:
    """返回 LanceDB 管理器（未初始化时为 None）。"""
    return _lancedb


def set_lancedb(mgr: LanceDBManager | None) -> None:
    """设置 LanceDB 管理器（lifespan 启动时调用一次；测试关闭/重启时允许 None）。"""
    global _lancedb
    _lancedb = mgr


def get_current_model() -> str | None:
    """返回当前 Embedding 模型名（未初始化时为 None）。"""
    return _current_model


def set_current_model(model: str) -> None:
    """设置当前模型（lifespan 启动时一次；T3.x 切换确认后也会写）。"""
    global _current_model
    _current_model = model


def get_current_embedding_version() -> int | None:
    """返回当前 Embedding 版本号（未初始化时为 None）。"""
    return _current_embedding_version


def set_current_embedding_version(version: int) -> None:
    """设置当前 Embedding 版本号。"""
    global _current_embedding_version
    _current_embedding_version = version


def reset_state() -> None:
    """重置全部状态（仅测试用，生产代码禁止调用）。"""
    global _psk, _last_seq, _used_nonces, _lancedb, _current_model, _current_embedding_version
    _psk = None
    _last_seq = 0
    _used_nonces = set()
    _lancedb = None
    _current_model = None
    _current_embedding_version = None
