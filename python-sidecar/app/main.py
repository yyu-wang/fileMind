"""FileMind Sidecar 入口：FastAPI 应用 + 生命周期管理。

启动流程：
1. ``lifespan`` 启动时从 stdin 读取 PSK（hex 编码），存入 ``app.state`` 模块
2. HMAC 中间件对每个非豁免路由验签 + 检查序号防重放
3. 握手路由 ``/handshake`` 完成 Sidecar 身份验证

安全映射：S-01（Sidecar 端口冒充）、T-01（Sidecar 通信篡改）。
"""

from __future__ import annotations

import os
import sys
from contextlib import asynccontextmanager
from typing import TYPE_CHECKING

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app import state
from app.api import (
    routes_chat,
    routes_classify,
    routes_embedding,
    routes_handshake,
    routes_health,
    routes_index,
    routes_metrics,
    routes_shutdown,
)
from app.middleware.hmac_auth import HMACMiddleware

if TYPE_CHECKING:
    from collections.abc import AsyncIterator


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    """应用生命周期钩子。

    startup：从 stdin 读取 PSK（hex 编码 64 字符 + 换行）。
        生产模式（Rust spawn）：stdin 是 pipe，Rust 通过 stdin 注入 PSK。
        dev 模式（用户直接启动 uvicorn）：stdin 是 tty，无 PSK 注入，
            中间件跳过验签（仅本机测试，发布版不会出现）。
        PyInstaller onefile 模式（sidecar_entry.py 负责注入）：
            ``PYINSTALLER_RUNTIME=1`` 且 ``state.get_psk()`` 已非空，跳过
            二次读 stdin（避免 onefile bootloader 复用 PIPE 时读阻塞）。

    shutdown：当前无特殊清理，Sidecar 由 Rust 端 ``SidecarManager`` kill。
    """
    if (
        os.environ.get("PYINSTALLER_RUNTIME") != "1" or state.get_psk() is None
    ) and not sys.stdin.isatty():
        # 非 PyInstaller 模式（dev / Rust 直接调 python -m uvicorn），或 PyInstaller
        # 模式但入口脚本未注入 PSK（fallback）时，stdin PIPE 首行是 PSK hex。
        psk_hex = sys.stdin.readline().strip()
        if psk_hex:
            state.set_psk(bytes.fromhex(psk_hex))
    # dev 模式：PSK 保持 None，中间件跳过验签
    yield


app = FastAPI(
    title="FileMind Sidecar",
    version="0.1.0",
    description="Python Sidecar for FileMind — classification + RAG + embedding",
    lifespan=lifespan,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:1420"],
    allow_methods=["*"],
    allow_headers=["*"],
)
# HMAC 验签 + 序号防重放中间件（豁免 /handshake、/health、文档路由）
app.add_middleware(HMACMiddleware)

app.include_router(routes_handshake.router)
app.include_router(routes_health.router)
app.include_router(routes_shutdown.router)
app.include_router(routes_metrics.router)
app.include_router(routes_classify.router)
app.include_router(routes_index.router)
app.include_router(routes_chat.router)
app.include_router(routes_embedding.router)
