"""父进程死亡看门狗：Sidecar 自识别孤儿身份并退出。

背景（macOS 真机复现的缺陷）：⌘Q 由 AppKit 的 ``-[NSApplication terminate:]`` 直接
``exit()``，Rust 进程被瞬间终结——事件循环、``RunEvent::ExitRequested``、``Drop``
**全部不会执行**，Sidecar 被 launchd 收养（``ppid == 1``）后继续占用 8765，只能靠
下次启动的 ``cleanup_orphan_sidecar`` 兜底。

父进程已消失时子进程没有任何存在意义，故由 Sidecar 自己轮询 ``getppid()`` 判定孤儿
身份并退出。该判定与父进程**以何种方式**死亡无关：⌘Q / SIGKILL / panic-abort /
系统注销均覆盖，这是 Rust 侧退出钩子做不到的。

平台差异：POSIX 下父进程退出后子进程被 init/launchd 收养，``ppid`` 变为 1；
Windows 无 reparent 语义（孤儿仍保留已死父进程的 PID），本判定不成立，故调用方
仅需在 POSIX 平台启用（见 ``app.main`` 的 lifespan）。
"""

from __future__ import annotations

import os
import signal
import threading
import time
from typing import TYPE_CHECKING

from app.core.logging import getLogger

if TYPE_CHECKING:
    from collections.abc import Callable

logger = getLogger()

#: 轮询间隔（秒）。权衡：越长则孤儿占住 8765 越久，越短则越多的无谓唤醒。
#: 2s 内必然自退，远短于用户可感知的「端口被占」窗口。
PARENT_POLL_INTERVAL_SECS: float = 2.0

#: SIGTERM 后仍未退出时的硬退宽限期（秒），兜底守住「父死子必退」的承诺。
HARD_EXIT_GRACE_SECS: float = 5.0

#: POSIX 孤儿进程的 ``ppid``：父进程退出后子进程被 init/launchd 收养。
ORPHAN_PPID: int = 1


def is_orphaned(ppid: int) -> bool:
    """判断 ``ppid`` 是否代表父进程已退出（被 init/launchd 收养）。

    Args:
        ppid: ``os.getppid()`` 的返回值。

    Returns:
        True 表示当前进程已成为孤儿。
    """
    return ppid == ORPHAN_PPID


def _hard_exit() -> None:
    """硬退兜底：跳过解释器清理直接退出，确保端口被释放。"""
    os._exit(0)


def _terminate_self() -> None:
    """优雅退出自身：先 SIGTERM 交给 uvicorn 关停，宽限期内未退则硬退。

    uvicorn 在 ``Server.run`` 中于主线程事件循环注册了 SIGTERM 处理器，优雅关停通常
    在数十毫秒内完成；``os._exit`` 兜底是为了守住本机制的硬承诺——父进程已死时子进程
    必须退出，不能因为关停卡住而继续占着 8765。
    """
    fallback = threading.Timer(HARD_EXIT_GRACE_SECS, _hard_exit)
    fallback.daemon = True
    fallback.start()
    os.kill(os.getpid(), signal.SIGTERM)


def watch_parent(
    *,
    poll_interval: float = PARENT_POLL_INTERVAL_SECS,
    getppid: Callable[[], int] = os.getppid,
    on_orphaned: Callable[[], None] = _terminate_self,
) -> None:
    """阻塞轮询父进程状态，判定为孤儿后执行 ``on_orphaned`` 并返回。

    由 ``app.main`` 的 lifespan 在守护线程中启动一次。

    Args:
        poll_interval: 轮询间隔（秒）。
        getppid: 取父进程 PID 的实现（可注入，便于测试）。
        on_orphaned: 判定为孤儿后的退出动作（可注入，便于测试）。
    """
    while True:
        time.sleep(poll_interval)
        ppid = getppid()
        if not is_orphaned(ppid):
            continue
        logger.warning("parent_watchdog.orphaned_exit", ppid=ppid)
        on_orphaned()
        return
