"""事件循环兜底异常处理器：把「客户端断连」噪声降级为 debug。

背景（2026-09-15 实机复现）：Rust 端 ``SidecarManager`` 每秒探活一次 ``/health``，连接被
回收时 Windows proactor 循环在 ``_ProactorBasePipeTransport._call_connection_lost`` 里抛
``ConnectionResetError``（WinError 10054）；asyncio 默认异常处理器按 ERROR 级别把**完整
traceback** 打到 stderr —— 侧车控制台窗口被刷屏，``logs/sidecar.log`` 也被污染。这类断连是
探活 / 客户端中途取消的**正常现象**，不代表侧车故障，故降级为 debug。

实现：在现有处理器外层包一层过滤器（uvicorn / asyncio 原有行为保持不变），只对
[`CLIENT_DISCONNECT_ERRORS`] 命中的异常提前返回，其余上下文原样委托。

平台差异：POSIX 上客户端断开多表现为 EOF（不触发本噪声），Windows proactor 循环才会把
RST 抛进回调；过滤器不区分平台——无噪声时等同于透传，POSIX 上同样安全。
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from app.core.logging import getLogger

if TYPE_CHECKING:
    import asyncio
    from collections.abc import Callable

    #: 事件循环异常处理器签名。返回类型取 ``object`` 而非 ``None``：与 asyncio
    #: ``get_exception_handler()`` 的返回值类型保持一致（处理器返回值本就被忽略），
    #: 否则包装后的处理器无法回传给 ``set_exception_handler``（mypy strict 不通过）。
    LoopExceptionHandler = Callable[[asyncio.AbstractEventLoop, dict[str, object]], object]

logger = getLogger()

#: 视为「客户端断连噪声」的异常类型：连接被重置 / 中止 / 半关闭。
CLIENT_DISCONNECT_ERRORS: tuple[type[BaseException], ...] = (
    ConnectionResetError,
    ConnectionAbortedError,
    BrokenPipeError,
)


class ClientDisconnectFilter:
    """事件循环异常处理器：过滤客户端断连噪声，其余委托给原处理器。

    实现为**可调用对象**（而非闭包），便于单测直接构造并断言委托行为。
    """

    def __init__(self, previous: LoopExceptionHandler | None) -> None:
        """记录被包裹的原处理器。

        Args:
            previous: 安装前循环上已有的异常处理器；``None`` 表示回落到
                ``loop.default_exception_handler``。
        """
        self._previous = previous

    def __call__(self, loop: asyncio.AbstractEventLoop, context: dict[str, object]) -> None:
        """处理一次循环异常上下文：断连噪声降级，其余原样委托。

        Args:
            loop: 抛出该异常的事件循环。
            context: asyncio 异常上下文（``exception`` / ``message`` / ``transport`` 等键）。
        """
        exc = context.get("exception")
        if isinstance(exc, CLIENT_DISCONNECT_ERRORS):
            logger.debug(
                "loop.client_disconnect_ignored",
                error=type(exc).__name__,
                detail=str(context.get("message", "")),
            )
            return
        if self._previous is not None:
            self._previous(loop, context)
        else:
            loop.default_exception_handler(context)


def install_client_disconnect_filter(loop: asyncio.AbstractEventLoop) -> None:
    """在给定事件循环上安装断连噪声过滤器（重复调用为 no-op）。

    由 ``app.main`` 的 lifespan 在 startup 阶段调用：此时循环已在运行，且覆盖后续整个
    服务期（uvicorn 单循环跑完整生命周期）。

    Args:
        loop: 正在运行的事件循环。
    """
    current = loop.get_exception_handler()
    if isinstance(current, ClientDisconnectFilter):
        return
    loop.set_exception_handler(ClientDisconnectFilter(current))
