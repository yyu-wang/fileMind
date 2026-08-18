"""FileMind Sidecar 可执行文件入口（PyInstaller --onefile 目标）。

启动顺序（与 `manager.rs start()` 保持协议等价）：
1. 读 `SIDECAR_PORT` 环境变量，默认 8765。
2. 注入 PSK（优先级从高到低）：
   a. `PSK_HEX` 环境变量（16 进制，64 字符 = 32 字节）
   b. ``sys.stdin`` 读取首行（Rust 侧 `Command::new().stdin(PIPE).write_all(psk_hex + b"\\n")`）
   c. 两者均无：保持 `state.psk = None`（dev 模式，HMAC 中间件跳过验签）
3. ``uvicorn.run("app.main:app", ...)`` 启动 FastAPI。

Security:
    - PSK 不落地文件、不通过命令行 argv 传递（命令行在 ps 可见）。
    - 仅用环境变量（进程级）或 stdin PIPE（父子进程私有）两种机制。
"""

from __future__ import annotations

import contextlib
import multiprocessing
import os
import sys
from typing import NoReturn

from uvicorn import run as uvicorn_run

from app import state

DEFAULT_PORT: int = 8765


def _resolve_port() -> int:
    raw = os.environ.get("SIDECAR_PORT")
    if raw is None:
        return DEFAULT_PORT
    try:
        p = int(raw)
    except ValueError as exc:
        # uvicorn 不会拿到错误端口，这里直接 exit(2)，避免静默回退默认值
        print(f"[sidecar] ERROR: SIDECAR_PORT={raw!r} 不是整数", file=sys.stderr)
        raise SystemExit(2) from exc
    if not 1 <= p <= 65535:
        print(f"[sidecar] ERROR: SIDECAR_PORT={p} 超出范围 (1-65535)", file=sys.stderr)
        raise SystemExit(2)
    return p


def _inject_psk() -> None:
    """按优先级写入 Sidecar PSK；空注入保留 None（dev 模式跳过验签）。

    PSK 环境变量取 `PSK_HEX`（主名，CI/调试友好）或 `FILEMIND_PSK`（与项目命名空间一致），
    两者等价；只要任一非空就走 env 分支（前者优先）。
    """
    # 选项 a：PSK_HEX / FILEMIND_PSK env
    env_psk = os.environ.get("PSK_HEX") or os.environ.get("FILEMIND_PSK")
    if env_psk:
        try:
            state.set_psk(bytes.fromhex(env_psk.strip()))
            return
        except ValueError as exc:
            print(f"[sidecar] ERROR: PSK_HEX 不是合法 hex: {exc}", file=sys.stderr)
            raise SystemExit(2) from exc

    # 选项 b：stdin（Rust manager.rs 主路径）
    # 关键安全：std::process::Stdio::piped() 不会 isatty()，所以正常走 readline 分支；
    # 但如果 Stdio::null()（如打包测试脚本里），isatty 也是 false，readline 会立即返回 ""
    # （EOF）→ 不阻塞，走 c 选项 dev 模式 = None。
    if not sys.stdin.isatty():
        try:
            line = sys.stdin.readline().strip()
        except Exception:  # noqa: BLE001 - onefile bootloader 可能将 stdin 设为 /dev/null，读可能失败
            line = ""
        if line:
            try:
                state.set_psk(bytes.fromhex(line))
            except ValueError as exc:
                print(f"[sidecar] ERROR: stdin PSK 不是合法 hex: {exc}", file=sys.stderr)
                raise SystemExit(2) from exc
            return

    # 选项 c：dev 模式 — state.psk = None（中间件会跳过 HMAC 验签）


def main() -> NoReturn:
    # PyInstaller + macOS (spawn start method) + multiprocessing 兼容：
    # 必须在入口第一时间 freeze_support，否则 uvicorn 用 workers=N 时会
    # "子进程重新执行 bootloader → 死循环"。见 PyInstaller docs:
    # recipes/multiprocess.html。同时为了避免 workers 参数触发 spawn，我们
    # 下方 uvicorn_run 不指定 workers（默认 1 worker，直跑 asyncio loop）。
    # 某些受限环境下 freeze_support 抛异常不阻塞启动
    with contextlib.suppress(Exception):
        multiprocessing.freeze_support()

    port = _resolve_port()
    _inject_psk()
    # 关键：传字符串 module 路径 "app.main:app" 而非 app 对象
    # 确保 PyInstaller 通过 collect_submodules 收集到 app.main.* 子依赖时一致
    #
    # 另外：显式不穿 workers 参数（默认单进程直跑，避免 multiprocessing.spawn 问题）。
    # 如未来需要多 worker，需同时开启 workers=N 并验证 PyInstaller freeze_support 生效。
    uvicorn_run(
        "app.main:app",
        host="127.0.0.1",
        port=port,
        log_level="warning",
        access_log=False,
    )
    # uvicorn.run 返回时进程结束（SIGINT / SIGTERM / POST /shutdown 触发 sys.exit）
    raise SystemExit(0)


if __name__ == "__main__":
    main()
