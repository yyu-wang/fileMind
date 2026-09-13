"""父进程死亡看门狗单元测试。

覆盖三条契约：
1. 孤儿判定只认「被 init/launchd 收养」的 ``ppid == 1``，正常 ppid 不得误判；
2. 判定为孤儿后必须执行退出动作，且看门狗线程随即结束；
3. 父进程仍存活时看门狗持续轮询、绝不触发退出。

自退动作（SIGTERM + 硬退兜底）在测试中用 monkeypatch 拦截真实信号，
避免 pytest 进程自杀；真机验证见 PR 描述。
"""

from __future__ import annotations

import os
import signal
import threading
from typing import TYPE_CHECKING

from app.core import parent_watchdog

if TYPE_CHECKING:
    from collections.abc import Callable

    import pytest


def _start_watchdog(
    getppid: Callable[[], int],
    on_orphaned: Callable[[], None],
) -> threading.Thread:
    """以极短轮询间隔启动看门狗线程（测试专用）。"""
    thread = threading.Thread(
        target=parent_watchdog.watch_parent,
        kwargs={"poll_interval": 0.01, "getppid": getppid, "on_orphaned": on_orphaned},
        daemon=True,
    )
    thread.start()
    return thread


def test_is_orphaned_only_matches_reparented_ppid() -> None:
    """只有 ``ppid == 1``（被 init/launchd 收养）算孤儿，其它值一律不算。"""
    assert parent_watchdog.is_orphaned(1) is True
    # 0 = 父进程 pid 未知（部分平台查询失败），不能当作孤儿误杀自己
    assert parent_watchdog.is_orphaned(0) is False
    assert parent_watchdog.is_orphaned(4242) is False


def test_watch_parent_fires_and_returns_on_orphan() -> None:
    """父进程消失（ppid 变 1）→ 执行退出动作并结束线程。"""
    fired = threading.Event()
    thread = _start_watchdog(lambda: 1, fired.set)

    assert fired.wait(timeout=2.0), "父进程消失后必须触发退出动作"
    thread.join(timeout=2.0)
    assert not thread.is_alive(), "触发退出动作后看门狗线程应结束"


def test_watch_parent_keeps_running_while_parent_alive() -> None:
    """父进程仍存活 → 持续轮询，不得触发退出（防误杀）。"""
    fired = threading.Event()
    thread = _start_watchdog(lambda: 4242, fired.set)

    assert not fired.wait(timeout=0.2), "父进程存活时不应触发退出动作"
    assert thread.is_alive(), "父进程存活时看门狗应继续轮询"


def test_terminate_self_sends_sigterm_and_arms_hard_exit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """自退动作：先向自身发 SIGTERM（交给 uvicorn 优雅关停），并武装硬退兜底。"""
    sent: list[tuple[int, int]] = []

    def _record_kill(pid: int, sig: int) -> None:
        sent.append((pid, sig))

    monkeypatch.setattr(parent_watchdog.os, "kill", _record_kill)
    # 宽限期压到 10ms，并替换硬退动作为「置事件」，避免真把 pytest 打死
    monkeypatch.setattr(parent_watchdog, "HARD_EXIT_GRACE_SECS", 0.01)
    hard_exit_fired = threading.Event()
    monkeypatch.setattr(parent_watchdog, "_hard_exit", hard_exit_fired.set)

    parent_watchdog._terminate_self()

    assert sent == [(os.getpid(), signal.SIGTERM)], "应向自身发 SIGTERM 触发优雅关停"
    assert hard_exit_fired.wait(timeout=2.0), "宽限期内未退出时必须硬退兜底"
