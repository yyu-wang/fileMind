"""Sidecar 启动/签名共享模块（go-no-go.py 与 benchmarks/rag_bench.py 复用）。

封装两类公共能力：
- ``DevSidecar``：dev 模式（venv python + uvicorn，stdin 注入 PSK）与打包态
  （PyInstaller onefile，env PSK）启动/停止；
- ``SignedClient``：对同一 Sidecar 实例发 HMAC 签名请求（``X-Signature`` +
  ``X-Request-Seq``），内部维护单调递增序号满足防重放（``hmac_auth.py`` 要求
  同一实例内序号严格递增）。

模块内不执行任何逻辑，仅供 import。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import httpx  # type: ignore[import-not-found]
    import psutil  # type: ignore[import-not-found]
except ImportError as exc:  # pragma: no cover - 缺依赖时直接提示退出
    print(
        f"[FATAL] 缺少依赖: {exc}。请在 venv 内: pip install httpx psutil",
        file=sys.stderr,
    )
    sys.exit(2)

ROOT_DIR: Path = Path(__file__).resolve().parent.parent
PYSIDE_DIR: Path = ROOT_DIR / "python-sidecar"
SIDECAR_PORT: int = 8765
SIDECAR_BASE: str = f"http://127.0.0.1:{SIDECAR_PORT}"
GLOBAL_PSK: bytes = bytes.fromhex("a" * 64)  # 32 字节固定 hex，用作 demo PSK


def _sign(psk: bytes, msg: str) -> str:
    """HMAC-SHA256 hex 签名（与 Sidecar ``hmac_auth.py`` 一致）。"""
    return hmac.new(psk, msg.encode("utf-8"), hashlib.sha256).hexdigest()


def _canonical(method: str, path: str, body: str, seq: int) -> str:
    """HMAC canonical string：``{method}|{path}|{body}|{seq}``。"""
    return f"{method}|{path}|{body}|{seq}"


def _free_port() -> bool:
    """8765 端口空闲（可启动新 Sidecar）。"""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(0.3)
        return s.connect_ex(("127.0.0.1", SIDECAR_PORT)) != 0


def _wait_health(timeout: float = 10.0) -> httpx.Response | None:
    """轮询 /health 直到返回 200 或超时。"""
    deadline = time.time() + timeout
    last: httpx.Response | None = None
    while time.time() < deadline:
        try:
            last = httpx.get(f"{SIDECAR_BASE}/health", timeout=0.5)
            if last.status_code == 200:
                return last
        except (httpx.HTTPError, ConnectionError):
            pass
        time.sleep(0.1)
    return last


class SignedClient:
    """对同一 Sidecar 实例发签名请求；每次请求自动推进序号。"""

    def __init__(self) -> None:
        self._seq = 0

    def request(
        self,
        method: str,
        path: str,
        body_obj: dict[str, Any] | None = None,
        timeout: float = 30.0,
    ) -> httpx.Response:
        """发送带 HMAC 签名的请求。

        Args:
            method: HTTP 方法（GET/POST）。
            path: 路由路径（如 ``/metrics``），验签用 ``request.url.path`` 需一致。
            body_obj: POST 请求体（compact JSON 签名 + 发送），GET 传 ``None``。
            timeout: 超时秒数（``/index/build`` 等重操作需放宽）。

        Returns:
            httpx 响应对象。
        """
        body_str = (
            json.dumps(body_obj, separators=(",", ":"), sort_keys=True)
            if body_obj is not None
            else ""
        )
        self._seq += 1
        headers = {
            "X-Signature": _sign(
                GLOBAL_PSK, _canonical(method, path, body_str, self._seq)
            ),
            "X-Request-Seq": str(self._seq),
        }
        kwargs: dict[str, Any] = {"headers": headers, "timeout": timeout}
        if body_str:
            kwargs["content"] = body_str
            headers["Content-Type"] = "application/json"
        return httpx.request(method, f"{SIDECAR_BASE}{path}", **kwargs)


@dataclass
class DevSidecar:
    """Sidecar 生命周期：dev（uvicorn + stdin PSK）或打包二进制（env PSK）。"""

    proc: subprocess.Popen[str] | None = None
    binary_path: Path | None = None

    def start(self, binary: Path | None = None) -> None:
        """启动 Sidecar；``binary`` 为打包产物时走 env-PSK 分支，否则 dev。"""
        self.binary_path = binary
        if binary is not None:
            self._start_packaged(binary)
        else:
            self._start_dev()

    # ---------- dev 模式（stdin PSK，与 Rust manager.rs start() 一致） ----------
    def _start_dev(self) -> None:
        if not _free_port():
            raise RuntimeError(
                f"端口 {SIDECAR_PORT} 已被占用，请先释放（可能残留 Sidecar 进程）"
            )
        cmd = [
            self._resolve_python(),
            "-m",
            "uvicorn",
            "app.main:app",
            "--host",
            "127.0.0.1",
            "--port",
            str(SIDECAR_PORT),
        ]
        self.proc = subprocess.Popen(
            cmd,
            cwd=str(PYSIDE_DIR),
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
            env={**os.environ, "PYTHONUNBUFFERED": "1"},
        )
        assert self.proc.stdin is not None
        self.proc.stdin.write(GLOBAL_PSK.hex() + "\n")
        self.proc.stdin.flush()

    # ---------- 打包模式（env PSK） ----------
    def _start_packaged(self, binary: Path) -> None:
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise RuntimeError(f"打包二进制不存在或不可执行: {binary}")
        if not _free_port():
            raise RuntimeError(
                f"端口 {SIDECAR_PORT} 已被占用，请先释放（可能残留 Sidecar 进程）"
            )
        env = {
            **os.environ,
            "PYINSTALLER_RUNTIME": "1",
            "FILEMIND_PSK": GLOBAL_PSK.hex(),
            "SIDECAR_PORT": str(SIDECAR_PORT),
        }
        self.proc = subprocess.Popen(
            [str(binary)],
            cwd=str(ROOT_DIR),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
            env=env,
        )

    @staticmethod
    def _resolve_python() -> str:
        """优先仓库根 ``<repo>/.venv/bin/python``，其次 ``python-sidecar/.venv``，最后 PATH。"""
        for candidate in (
            ROOT_DIR / ".venv" / "bin" / "python",
            PYSIDE_DIR / ".venv" / "bin" / "python",
        ):
            if candidate.is_file():
                return str(candidate)
        found = shutil.which("python3") or shutil.which("python")
        if found:
            return found
        raise RuntimeError(
            "找不到 python3 / python，且 .venv 不存在；请先运行 scripts/bootstrap.sh"
        )

    def stop(self) -> None:
        """终止进程（terminate → 超时 kill），清理失败静默忽略。"""
        if self.proc is None:
            return
        try:
            if psutil.pid_exists(self.proc.pid):
                self.proc.terminate()
                self.proc.wait(timeout=3)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            try:
                self.proc.kill()
                self.proc.wait(timeout=2)
            except Exception:  # noqa: S110, BLE001 - 清理路径失败直接忽略，脚本级兜底不需日志
                pass
        finally:
            self.proc = None
