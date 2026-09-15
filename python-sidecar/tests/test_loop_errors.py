"""``app.core.loop_errors`` 单测：断连噪声降级 + 其余异常原样委托。

覆盖点（2026-09-15 实机噪声：Windows proactor 循环把客户端断连抛进回调）：
1. ConnectionResetError 上下文被吸收，不落到原处理器
2. 其他异常 / 无异常上下文原样委托（不误吞真实故障）
3. 重复安装幂等（lifespan 可能被多次进入）
"""

from __future__ import annotations

import asyncio
import contextlib
from typing import TYPE_CHECKING

from app.core.loop_errors import ClientDisconnectFilter, install_client_disconnect_filter

if TYPE_CHECKING:
    from collections.abc import Iterator


@contextlib.contextmanager
def _loop_with_spy() -> Iterator[tuple[asyncio.AbstractEventLoop, list[dict[str, object]]]]:
    """构造「装了 spy 原处理器」的事件循环，供各用例断言委托行为。"""
    loop = asyncio.new_event_loop()
    delegated: list[dict[str, object]] = []
    loop.set_exception_handler(lambda _loop, ctx: delegated.append(ctx))
    try:
        yield loop, delegated
    finally:
        loop.close()


def test_client_disconnect_is_absorbed() -> None:
    """客户端断连噪声不落到原处理器（降级为 debug）。"""
    with _loop_with_spy() as (loop, delegated):
        install_client_disconnect_filter(loop)
        loop.call_exception_handler(
            {"message": "reset", "exception": ConnectionResetError(10054, "reset")}
        )
        assert delegated == []


def test_other_exceptions_are_delegated() -> None:
    """真实故障（非断连）必须原样委托，不能被过滤器吞掉。"""
    with _loop_with_spy() as (loop, delegated):
        install_client_disconnect_filter(loop)
        loop.call_exception_handler({"message": "boom", "exception": ValueError("x")})
        assert len(delegated) == 1
        assert delegated[0]["message"] == "boom"


def test_context_without_exception_is_delegated() -> None:
    """无 exception 的上下文（asyncio 只用 message）同样委托。"""
    with _loop_with_spy() as (loop, delegated):
        install_client_disconnect_filter(loop)
        loop.call_exception_handler({"message": "slow callback"})
        assert len(delegated) == 1


def test_install_is_idempotent() -> None:
    """重复安装不叠加包装层（lifespan 可能被多次进入）。"""
    with _loop_with_spy() as (loop, _):
        install_client_disconnect_filter(loop)
        first = loop.get_exception_handler()
        install_client_disconnect_filter(loop)
        assert isinstance(first, ClientDisconnectFilter)
        assert loop.get_exception_handler() is first
