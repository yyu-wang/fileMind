# -*- mode: python ; coding: utf-8 -*-
"""T1.2 / P2-2 PyInstaller spec — FileMind Sidecar ``--onedir`` 可执行目录。

P2-2（2026-09-11）由 ``--onefile`` 改为 ``--onedir``：
    产物 = ``dist/filemind-sidecar/`` 目录（主可执行 + ``_internal/``）。
    动机：onefile 每次启动都要把 ~320MB 归档解压到临时目录，实测冷启动
    21s / 热盘 16s 中约 14s 消耗在解压；onedir 原地读取，消除该开销。
    代价：Tauri 集成由 ``externalBin`` 改为 ``bundle.resources`` 携带整个目录
    （见 src-tauri/tauri.conf.json + sidecar/manager.rs 的目录探测）。

体积门控：≤1600MB，按**目录逻辑大小**（`find -type f` 逐个 size 求和，不跟随 symlink）
    统计。本机实测：**源产物 915MB / 6136 文件**；经 Tauri `copy_resources` 打包后约
    1384MB / 6172 文件（该步骤会把 `Python.framework/Versions/Current` 等 36 个 symlink
    **解引用**成真实文件副本）。两者共用 1600MB 门控，均 PASS。
    口径沿革：早期 E1 PoC 不含向量/重排依赖时 onefile 仅约 24MB、门控 80MB；引入 lancedb
    （+pyarrow/lance）、numpy、jieba、ollama/openai、sentence-transformers（离线
    embedding/rerank，拖入 torch）等**运行时硬依赖**后，onefile 压缩态约 315MB、
    门控上调至 400MB。P2-2 改为 onedir 后产物是解压落地态，按实测绝对值设 1600MB——
    用户实际下载体积（tar.gz / dmg / msi 压缩后）实测 317MB，与原 onefile 持平。
    ⚠️ 不用 `du` 统计：APFS 上 clone/硬链接共享块会让同一棵树的不同副本给出
    不一致读数，无法作为跨副本可比的门控口径。

体积优化策略（当前有效）：
1. **``optimize=1``**：``-O`` 级别（仅删 assert，**保留 docstring**——transformers
   5.x 运行时解析 docstring，``optimize=2`` 会导致 rerank 模型加载失败，见下方
   ``Analysis(optimize=...)`` 处注释）。
2. **``strip=True``**：剥离 native 扩展 / stdlib 符号（EXE 与 COLLECT 均设）。
3. **``excludes`` 仅剔除确认用不到的冗余**：tkinter/venv/lib2to3 等 stdlib 与
   pytest/ruff/mypy 等 dev 依赖（运行依赖一律不排除）。
4. **``upx=False``**：UPX 与 ``_lancedb.abi3.so``、``_pydantic_core`` 等 native
   扩展常见冲突，不启用。

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
    "app.api.routes_preview",
    "app.api.routes_chat",
    "app.api.routes_embedding",
    "app.middleware",
    "app.middleware.hmac_auth",
    # P2-1：以下两个重依赖已改为「函数内惰性导入」（缩短启动期 import 耗时），
    # 虽仍可被字节码扫描命中，但为杜绝打包态 ModuleNotFound（该风险仅在打包后
    # 暴露、dev/单测无法发现），显式登记兜底。
    "lancedb",
    "openai",
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
    # optimize=1（-O）：仅去 assert，保留 docstring。曾用 2（-OO，连 docstring
    # 一起剥离）压缩体积，但 transformers 5.x 加载 CrossEncoder 时需在运行时
    # 解析 ModelOutput 的 docstring（auto_docstring.py），docstring 被剥离会抛
    # 「No `Args` or `Parameters` section is found」→ rerank 永远不可用
    # （dev 直跑 venv 未剥离所以无此错误；已用 `python -OO` 复现实锤）。
    optimize=1,
)

pyz = PYZ(a.pure)

# P2-2 onedir：EXE 只含 bootloader + PYZ；二进制作业/数据交给 COLLECT 落到
# 同级的 ``_internal/``，启动时原地读取（不再解压到临时目录）。
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="filemind-sidecar",
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,
    upx=False,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=_PYI_TARGET_ARCH,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=True,
    upx=False,
    upx_exclude=[],
    name="filemind-sidecar",
)
