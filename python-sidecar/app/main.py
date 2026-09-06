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
from pathlib import Path
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
    routes_inference,
    routes_metrics,
    routes_preview,
    routes_search,
    routes_shutdown,
)
from app.core import embedding_models
from app.core.logging import getLogger
from app.db.lancedb_repo import LanceDBManager
from app.middleware.hmac_auth import HMACMiddleware

if TYPE_CHECKING:
    from collections.abc import AsyncIterator

# T10.3：OMP 线程帽 —— 必须在任何 import torch 之前 setdefault。
# rerank 首次加载时 torch 默认占用全部逻辑核心，与 Tauri 主进程争抢 CPU；
# 默认 cap 到 (核心数 - 1)，用户可用 OMP_NUM_THREADS 显式覆盖（setdefault 不覆盖）。
# 全仓 torch 仅在 ``rerank_service._load_cross_encoder`` 惰性导入（已 grep 验证）。
os.environ.setdefault("OMP_NUM_THREADS", str(max(1, (os.cpu_count() or 1) - 1)))

logger = getLogger()

# 默认 Embedding 模型：bge-large-zh-v1.5（1024 维，中文场景下语义向量 SOTA）
# 与 E2 后续任务 T2.6（模型切换流程）的"初始默认表"保持一致
DEFAULT_EMBEDDING_MODEL = "bge-large-zh-v1.5"
DEFAULT_EMBEDDING_DIM = 1024
DEFAULT_EMBEDDING_VERSION = 1

# 数据根目录：与 SQLite (~/.filemind/data/filemind.db) 同层
DATA_HOME = Path(os.environ.get("FILEMIND_DATA_HOME", str(Path.home() / ".filemind")))
LANCEDB_HOME = DATA_HOME / "data" / "lancedb"


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    """应用生命周期钩子。

    startup 顺序（按依赖顺序执行，异常不阻塞 Core，但会记录 warning）：
        1. 读 PSK（stdin 注入 / PyInstaller onefile 入口已注入两种情形）
        2. 初始化 LanceDB：目录+权限 + 默认模型表 ensure_table

    shutdown：当前无特殊清理，Sidecar 由 Rust 端 ``SidecarManager`` kill。
    """
    # --- 步骤 1：PSK 注入 -------------------------------------------------
    if (
        os.environ.get("PYINSTALLER_RUNTIME") != "1" or state.get_psk() is None
    ) and not sys.stdin.isatty():
        # 非 PyInstaller 模式（dev / Rust 直接调 python -m uvicorn），或 PyInstaller
        # 模式但入口脚本未注入 PSK（fallback）时，stdin PIPE 首行是 PSK hex。
        psk_hex = sys.stdin.readline().strip()
        if psk_hex:
            # SC-m20：hex 非法时不应崩 lifespan，跳过 PSK（dev 模式中间件跳过验签）
            try:
                state.set_psk(bytes.fromhex(psk_hex))
            except ValueError:
                logger.error("main.psk_invalid_hex", psk_len=len(psk_hex))
    # dev 模式：PSK 保持 None，中间件跳过验签

    # --- 步骤 2：LanceDB 初始化（T2.2 新增） ------------------------------
    # 异常不阻塞 Sidecar 启动：索引/查询功能降级报错，健康检查仍通过
    try:
        mgr = LanceDBManager(LANCEDB_HOME)
        mgr.connect()
        default_table = mgr.ensure_table(
            DEFAULT_EMBEDDING_MODEL,
            DEFAULT_EMBEDDING_VERSION,
            DEFAULT_EMBEDDING_DIM,
        )
        state.set_lancedb(mgr)
        # 把「当前模型 + 当前版本」写入 state（注册表单一来源）
        # T2.6 不做状态改变——只在启动时注入初始值
        state.set_current_model(embedding_models.DEFAULT_MODEL)
        state.set_current_embedding_version(DEFAULT_EMBEDDING_VERSION)
        logger.info(
            "lancedb.ready",
            home=str(LANCEDB_HOME),
            default_table=default_table,
            model=DEFAULT_EMBEDDING_MODEL,
            version=DEFAULT_EMBEDDING_VERSION,
            dim=DEFAULT_EMBEDDING_DIM,
        )
    except Exception as exc:  # noqa: BLE001
        # LanceDB 初始化失败 → 记录日志 + 让 state 保持 None，后续路由在
        # get_lancedb() is None 时抛 503 Service Unavailable（T2.3/T2.5 时统一）
        logger.warning("lancedb.init_failed", home=str(LANCEDB_HOME), error=str(exc))
        state.set_lancedb(None)

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
app.include_router(routes_inference.router)
app.include_router(routes_preview.router)
app.include_router(routes_chat.router)
app.include_router(routes_embedding.router)
app.include_router(routes_search.router)
