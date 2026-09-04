# -*- mode: python ; coding: utf-8 -*-
"""T1.2 PyInstaller spec — FileMind Sidecar ``--onefile`` 可执行。

体积门控（2026-09-04 修订）：≤400MB。历史：早期 E1 PoC 不含向量/重排依赖，
onefile 仅约 24MB、门控 80MB；引入 lancedb（+pyarrow/lance）、numpy、jieba、
ollama/openai、sentence-transformers（离线 embedding/rerank，拖入 torch）等
**运行时硬依赖**后，真实 onefile 体积约 315MB（本机 aarch64 实测 2026-08-24），
无法靠 excludes 压回 80MB。决策记录：docs/packaging-implementation-plan.md §1 D1。

体积优化策略（当前有效）：
1. **``optimize=2``**：``.pyo`` 级别（删除 docstring + assert）。
2. **``strip=True``**：剥离 native 扩展 / stdlib 符号。
3. **``excludes`` 仅剔除确认用不到的冗余**：tkinter/venv/lib2to3 等 stdlib 与
   pytest/ruff/mypy 等 dev 依赖（运行依赖一律不排除）。
4. **``upx=False``**：UPX 与 ``_lancedb.abi3.so``、``_pydantic_core`` 等 native
   扩展常见冲突，不启用。
5. 后续优化候选（本轮不做）：改 ``--onedir`` 缩短冷启动（当前 onefile 冷启动约
   39s），需同步调整 Tauri externalBin 集成方式。

跨平台：target_arch 由 ``scripts/build-sidecar.sh`` 用 ``PYINSTALLER_TARGET_ARCH`` 传参，
此处不硬编码；``name="filemind-sidecar"`` 统一（Windows 会自动加 .exe 后缀）。
"""

from __future__ import annotations

import os

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

# PyInstaller 6.x 不支持 `pyinstaller specfile --target-arch=X`，
# 所以通过环境变量 ``PYINSTALLER_TARGET_ARCH`` 由 build-sidecar.sh 传进 spec。
# arm64 / x86_64 / ia32，None = 当前 Python 解释器架构。
_PYI_TARGET_ARCH = os.environ.get("PYINSTALLER_TARGET_ARCH") or None

# --- hidden imports --------------------------------------------------------
# 被 uvicorn / starlette / fastapi / httpx / pydantic v2 动态加载，
# 不能靠 AST import 扫描命中，显式列出防止打包后 ModuleNotFound。
_hidden: list[str] = [
    *collect_submodules("uvicorn.loops"),
    *collect_submodules("uvicorn.protocols"),
    *collect_submodules("uvicorn.lifespan"),
    *collect_submodules("uvicorn.logging"),
    *collect_submodules("starlette.middleware"),
    *collect_submodules("starlette.routing"),
    *collect_submodules("starlette.responses"),
    *collect_submodules("fastapi.responses"),
    *collect_submodules("fastapi.exceptions"),
    *collect_submodules("fastapi.dependencies"),
    *collect_submodules("fastapi.params"),
    *collect_submodules("httpx._transports"),
    *collect_submodules("httpcore._async"),
    *collect_submodules("httpcore._backends"),
    *collect_submodules("httpcore._sync"),
    # 自项目全量（app.main:app 字符串入口，需保证所有子路由收集到）
    "app",
    "app.main",
    "app.state",
    "app.models",
    "app.api",
    "app.api.routes_health",
    "app.api.routes_handshake",
    "app.api.routes_shutdown",
    "app.api.routes_metrics",
    "app.api.routes_classify",   # 只挂路由，不调用实现
    "app.api.routes_index",
    "app.api.routes_chat",
    "app.api.routes_embedding",
    "app.middleware",
    "app.middleware.hmac_auth",
    # pydantic v2 native 扩展
    "pydantic",
    "pydantic_core",
    # stdlib 常被 fastapi/starlette 间接通过 __import__ 调
    "mimetypes",
    "email",
    "email.utils",
    "email._parseaddr",
    "json",
    "hashlib",
    "hmac",
    "uuid",
    "secrets",
    "asyncio",
    "asyncio.tasks",
    "asyncio.runners",
    "concurrent",
    "concurrent.futures",
    "multiprocessing",
    "threading",
    "logging",
    "logging.config",
    "logging.handlers",
    "tracemalloc",
]

# --- datas ------------------------------------------------------------------
# 非代码资源文件。打包态侧车经 PyInstaller 解压到 _MEI 临时目录，__file__ 相对
# 路径不再指向源码树，必须显式收集数据文件；否则 /classify（规则引擎）在打包态
# 会 FileNotFoundError: preset_rules.json。
_datas: list[tuple[str, str]] = [
    *collect_data_files("app.rules.presets", include_py_files=False),
    # TBD（后续启用 jieba 分词后加回）:
    #   *collect_data_files("jieba", include_py_files=False,
    #                        excludes=["*.pyc", "__pycache__"]),
]

# --- excludes ---------------------------------------------------------------
# 仅剔除确认用不到的冗余（stdlib 开发组件 + dev 依赖）。运行依赖（lancedb/numpy/
# jieba/sentence-transformers 等）是 Sidecar 功能硬需求，一律保留。说明见文件头。
_excludes: list[str] = [
    # Tkinter / IDLE / ensurepip / venv / lib2to3 — stdlib 冗余（打包 never 用）
    "tkinter",
    "tkinter.ttk",
    "turtle",
    "idlelib",
    "ensurepip",
    "venv",
    "lib2to3",
    # pytest / dev 依赖（绝不会被生产代码 import，防误收）
    "pytest",
    "_pytest",
    "ruff",
    "mypy",
]


a = Analysis(
    ["sidecar_entry.py"],
    pathex=["."],
    binaries=[],
    datas=_datas,
    hiddenimports=_hidden,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=_excludes,
    noarchive=False,
    optimize=2,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name="filemind-sidecar",
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=_PYI_TARGET_ARCH,
    codesign_identity=None,
    entitlements_file=None,
)
