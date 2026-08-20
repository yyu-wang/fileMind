# -*- mode: python ; coding: utf-8 -*-
"""T1.2 PyInstaller spec — FileMind Sidecar ``--onefile`` 可执行（体积硬目标 <80MB）。

体积优化策略（优先级从高到低）：
1. **``exclude`` 大模块**：lancedb（128.9MB）、pyarrow、lance、ollama、jieba、
   ``numpy`` 子包。E1 阶段 Sidecar 仅承担 HMAC / 握手 / IPC / 健康检查 / 分类规则
   路由（不含真实分类引擎调用），不需要这些重依赖；T4/T5 阶段才懒加载导入。
2. **``optimize=2``**：``.pyo`` 级别（删除 docstring + assert）。
3. **``strip=True``**：从 ELF/Mach-O 剥离 debug 符号（省 ~15MB stdlib+pydantic_core 符号）。
4. **``upx=False``**：UPX 与 ``_lancedb.abi3.so``、``_pydantic_core.cpython-*.so`` 等
   native 扩展常见冲突；先不用，若超标再按模块启用。

跨平台：target_arch 由 ``scripts/build-sidecar.sh`` 用 ``--target-arch arm64/x86_64`` 传参，
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
# 非代码资源文件。jieba 先在 exclude 里，所以 datas 先留空；T4.3 后再补。
_datas: list[tuple[str, str]] = [
    # TBD（T4.3 分类层启用 jieba 后加回）:
    #   *collect_data_files("jieba", include_py_files=False,
    #                        excludes=["*.pyc", "__pycache__"]),
]
# 保留 collect_data_files import 防止 ruff 清理（T4.3 复用）
_ = collect_data_files

# --- excludes ---------------------------------------------------------------
# 删除这些模块，是 <80MB 的关键。说明见文件头。
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
