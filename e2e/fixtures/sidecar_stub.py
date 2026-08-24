#!/usr/bin/env python3
"""T9.5 — CI E2E 冒烟的 HMAC 合规 Sidecar 桩（仅限 CI 确定性冒烟）。

与真实 sidecar 的行为对齐（python-sidecar/app 协议）：
  1. stdin 首行读 PSK hex（Rust `SidecarManager::start` 注入）。
  2. `GET /health` → 200（`wait_ready` 仅要求 2xx 成功）。
  3. `POST /handshake` → 验签（豁免路径，但按协议校验）+ 返回
     `proof = HMAC-SHA256(psk, "handshake-ok|{nonce}")`。
  4. `POST /shutdown` → 200（应用退出时请求）。
  5. 其余路由（/classify 等）→ 404。

为什么存在：main.rs 无 sidecar 握手即退出，CI 必须有可执行 sidecar。
E2E-001/002 是确定性路径：扫描/分类/执行/撤销均为 Rust 本地逻辑，
fixture 保证零「待确认」→ 不触发 /classify；/index/* 是 best-effort 静默失败。
这是对「不 Mock 后端」的 CI-only 偏离，仅限冒烟；完整忠实 sidecar 留给
nightly/全量 job（用真实 PyInstaller 构建）。

用法：chmod +x sidecar_stub.py；`FILEMIND_SIDECAR_BINARY=sidecar_stub.py`。
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

#: 端口与 Rust `SIDECAR_PORT` 对齐（main.rs 会以 env 注入）。
PORT = int(os.environ.get("SIDECAR_PORT", "8765"))


def _sign(psk: bytes, message: str) -> str:
    return hmac.new(psk, message.encode("utf-8"), hashlib.sha256).hexdigest()


def _read_psk() -> bytes:
    """stdin 首行读 PSK hex（与 sidecar lifespan 一致；无输入则降级 None）。"""
    line = sys.stdin.readline().strip()
    if line:
        return bytes.fromhex(line)
    return b""


_PSK = _read_psk()


class Handler(BaseHTTPRequestHandler):
    """HTTP 桩：按 Sidecar 协议处理 health / handshake / shutdown。"""

    def log_message(self, fmt: str, *args: object) -> None:  # 静默，避免污染 wdio 输出
        return

    # -- 响应助手 ---------------------------------------------------------

    def _json(self, code: int, payload: dict[str, object]) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _ok(self) -> None:
        self._json(200, {"status": "ok", "version": "0.1.0", "uptime_seconds": 0.0})

    # -- 路由 ------------------------------------------------------------

    def _route(self) -> None:
        path = self.path.split("?", 1)[0]
        if self.command == "GET" and path == "/health":
            self._ok()
            return
        if self.command == "POST" and path == "/handshake":
            self._handshake()
            return
        if self.command == "POST" and path == "/shutdown":
            self._ok()
            return
        self._json(404, {"detail": "not implemented in stub"})

    def _handshake(self) -> None:
        """验签（canonical `handshake|{nonce}`）+ 返回 proof。"""
        try:
            length = int(self.headers.get("Content-Length", "0"))
            body = json.loads(self.rfile.read(length).decode("utf-8") or "{}")
            nonce = str(body.get("nonce", ""))
        except (ValueError, json.JSONDecodeError):
            self._json(400, {"detail": "bad request"})
            return

        signature = self.headers.get("X-Signature", "")
        if _PSK and _sign(_PSK, f"handshake|{nonce}") != signature:
            self._json(401, {"detail": "signature mismatch"})
            return
        self._json(200, {"proof": _sign(_PSK, f"handshake-ok|{nonce}") if _PSK else ""})

    # -- 协议入口 ---------------------------------------------------------

    def do_GET(self) -> None:  # noqa: N802（http.server 命名约定）
        self._route()

    def do_POST(self) -> None:  # noqa: N802
        self._route()


def main() -> None:
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
