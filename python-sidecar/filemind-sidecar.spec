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

Windows 专项修复（2026-09-15，本机 Windows 11 26100 实机排障；见文件末尾「Windows 两处
打包缺陷」注释块）：
1. **不进包构建机 CPython 自带的向下兼容 UCRT**（``ucrtbase.dll``）：它会被 PyInstaller
   主动加载（bootloader 源码 pyi_pythonlib.c 有专为此写的 ``pyi_utils_dlopen(ucrtbase)``），
   在系统 UCRT 比构建机新的目标机上会让 ``python312.dll`` 加载失败 →
   ``[PYI-1628:ERROR] Failed to load Python DLL ... LoadLibrary: 找不到指定的模块``。
2. **Windows 关闭 ``strip``**：CI 在 Git Bash（MSYS）里构建，PATH 中有 binutils
   ``strip.exe``，PyInstaller 的 ``strip=True`` 会真的对每个 DLL/.pyd 执行它，
   把 PE 尾部 Authenticode 签名截断却留下指向 EOF 之外的证书表目录项；打包产物里
   ``libssl-3.dll`` 已实测在装载时因重定位处理崩溃（Win32 998 ``内存位置访问无效``），
   导致 ``_ssl`` 导入失败、侧车启动即退出。
"""

from __future__ import annotations

import os
import sys

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

# --- Windows 两处打包缺陷的修复 ------------------------------------------------
# 缺陷 1：构建机（actions/setup-python 的 toolcache CPython）在安装目录里自带一份
#   「向下兼容 UCRT」（ucrtbase.dll + api-ms-win-crt-*.dll 转发桩），PyInstaller 会把它
#   当普通依赖收进 _internal。bootloader 启动时**主动** dlopen 这份 ucrtbase.dll
#   （pyi_pythonlib.c：为「目标机未装 UCRT 更新」的老系统准备），于是后面 python312.dll
#   的 api-ms-win-crt-* → ucrtbase 绑定到这份**比目标机系统更旧**的副本上，加载直接失败
#   （实测：CI 产物 ucrtbase 10.0.26100.1742 vs 本机系统 10.0.26100.9444）。
#   修复：Windows 明确剔除它，改用目标机系统 UCRT（Win10+ 是 Tauri 2 的下限，必然具备）。
#   api-ms-win-crt-*.dll 转发桩保留（实测无害：无本包 ucrtbase 时经系统 API set 解析）。
#   验证方式见文件末尾注释块。
_WIN_BIN_EXCLUDES: frozenset[str] = frozenset({"ucrtbase.dll"})
if sys.platform.startswith("win"):
    _dropped_bins = sorted(
        os.path.basename(entry[0]).lower()
        for entry in a.binaries
        if os.path.basename(entry[0]).lower() in _WIN_BIN_EXCLUDES
    )
    if _dropped_bins:
        print(f"[spec] Windows：剔除构建机自带 UCRT 副本 {sorted(set(_dropped_bins))}")
    a.binaries = [
        entry for entry in a.binaries if os.path.basename(entry[0]).lower() not in _WIN_BIN_EXCLUDES
    ]

# 缺陷 2：Windows 关闭 strip。CI（merge-build.yml）用 `shell: bash` 在 Git Bash 里跑
#   build-sidecar.sh，PATH 中带 MSYS 的 strip.exe，因此 PyInstaller 的 strip=True 在
#   Windows 上**真的会执行** binutils strip（PyInstaller 文档本就标注 Windows 不推荐）。
#   实测后果：产物目录里 300+ 个 DLL/.pyd 全部丢了尾部 Authenticode 签名，但 PE
#   data directory 的 security 项仍指向 EOF 之外的偏移（SecOffset == 文件长度），
#   且 libssl-3.dll 在装载时重定位处理崩溃（Win32 998「内存位置访问无效」）→
#   `import _ssl` 失败 → 侧车启动即退出。macOS 保留 strip（既有产物已实机验证可用）。
_STRIP: bool = not sys.platform.startswith("win")

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
    strip=_STRIP,
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
    strip=_STRIP,
    upx=False,
    upx_exclude=[],
    name="filemind-sidecar",
)

# ---------------------------------------------------------------------------
# Windows 两处打包缺陷：现象、复现与回归验证（2026-09-15 实机排障记录）
# ---------------------------------------------------------------------------
# 现象（用户安装到 D:\FileMind 后双击 filemind.exe）：
#   a. 状态栏/问答页永久停在「AI 引擎启动中，就绪后可开始问答…」；
#   b. 弹出控制台窗口并打印
#      [PYI-1628:ERROR] Failed to load Python DLL
#      'D:\FileMind\sidecar\_internal\python312.dll'. LoadLibrary: 找不到指定的模块。
#
# 复现（无需起 Tauri）：直接跑安装目录里的侧车主程序，stderr 即上述 PYI 报错：
#   PS> D:\FileMind\sidecar\filemind-sidecar.exe
#
# 定位结论（本机实测，两条独立缺陷）：
#   1. 把 _internal\ucrtbase.dll 改名后，bootloader 立刻能加载 python312.dll 并进入
#      Python（说明 DLL 本身与依赖都完好，问题在这份「构建机旧版 UCRT 副本」）；
#   2. 去掉它之后 Python 起得来但倒在 `import _ssl`：998 = ERROR_NOACCESS
#      「内存位置访问无效」；单独验证 `_ssl.pyd` 的依赖链，最终锁定 _internal\libssl-3.dll
#      —— 该文件 LoadLibraryEx(默认) 必失败 998，LoadLibraryEx(LOAD_LIBRARY_AS_DATAFILE)
#      正常（镜像可映射），清掉其 reloc 目录项后又能正常装载 ⇒ 装载期重定位处理崩溃，
#      即 PE 镜像被 strip 类工具改坏；全目录 300+ 个 DLL 的 security 目录项都指向 EOF
#      之外，是同一批 strip 操作的指纹。
#
# 回归验证（Windows 打包后必做，CI 目前只做「文件存在」结构层校验，抓不到本类问题）：
#   1. 产物目录里 `dir _internal\ucrtbase.dll` 应不存在（本 spec 已剔除）；
#   2. 直接运行 `dist\filemind-sidecar\filemind-sidecar.exe`：不应出现 PYI-1628，
#      且应稳定跑起来（stderr 无 ImportError: DLL load failed while importing _ssl）；
#   3. 冒烟：设 PSK_HEX=<64 hex> SIDECAR_PORT=8799 后 `GET /health` 应 200。
#   4. 校验签名未被破坏：
#      `Get-ChildItem _internal -Include *.dll,*.pyd -Recurse | Where-Object {
#         $b=[IO.File]::ReadAllBytes($_.FullName); $pe=[BitConverter]::ToInt32($b,0x3C);
#         [BitConverter]::ToInt32($b,$pe+24+112+32) -eq $b.Length } | Measure-Object`
#      期望 Count = 0（=0 表示没有「证书表指向 EOF 之外」的残留）。

