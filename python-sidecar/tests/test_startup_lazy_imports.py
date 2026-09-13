"""P2-1 启动性能回归：重依赖必须惰性导入，不得拖慢 Sidecar 启动。

背景：``lancedb``（冷导入约 0.7s）与 ``openai``（约 0.3s）原由 ``app.main``
启动期直接导入，占 ``import app.main`` 总耗时（1.65s）中的约 1.0s；uvicorn 必须
等该导入与应用 lifespan 完成才开始服务 ``/health``，因此这段耗时直接推迟 Rust
侧握手完成（AI 引擎可用时间）。

约束：本测试必须在**子进程**中执行——pytest 自身与其他用例可能已导入这些模块，
主进程的 ``sys.modules`` 无法反映「导入 ``app.main`` 时的真实依赖集」。
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

#: python-sidecar 根目录（``app`` 包所在目录 = ``tests/`` 的父目录）
_SIDECAR_ROOT = Path(__file__).resolve().parent.parent

#: 启动期不得加载的重依赖（惰性化目标）
_LAZY_ONLY_MODULES = ("lancedb", "openai", "numpy", "torch")


def _run_in_subprocess(code: str) -> subprocess.CompletedProcess[str]:
    """在 sidecar 根目录下用全新解释器执行 ``code``（隔离 ``sys.modules``）。"""
    return subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        cwd=str(_SIDECAR_ROOT),
        check=False,
    )


def test_import_app_main_does_not_load_heavy_deps() -> None:
    """导入 ``app.main``（uvicorn 启动前必经）不得加载重依赖。"""
    code = (
        "import sys\n"
        "import app.main\n"
        f"_LAZY = {list(_LAZY_ONLY_MODULES)!r}\n"
        "leaked = [m for m in _LAZY if m in sys.modules]\n"
        "assert not leaked, f'启动期不应加载重依赖: {leaked}'\n"
    )
    result = _run_in_subprocess(code)
    assert result.returncode == 0, f"惰性导入被破坏:\n{result.stderr}"


def test_lazy_deps_still_load_on_actual_use() -> None:
    """惰性化不得导致「该用时加载不了」：真实使用路径仍能正常加载。"""
    code = (
        "import sys\n"
        "from app.services.providers.openai_provider import OpenAIProvider\n"
        "assert 'openai' not in sys.modules, '仅导入 provider 模块不应加载 openai'\n"
        "OpenAIProvider(model='gpt-4o', token='t')\n"
        "assert 'openai' in sys.modules, '实例化后 openai 应已加载'\n"
        "msgs = OpenAIProvider._build_messages('s', 'u')\n"
        "assert msgs[0]['role'] == 'system' and msgs[1]['role'] == 'user'\n"
        "from app.db.lancedb_repo import LanceDBManager\n"
        "assert 'lancedb' not in sys.modules, 'LanceDBManager 导入本身应保持惰性'\n"
    )
    result = _run_in_subprocess(code)
    assert result.returncode == 0, f"惰性加载使用路径失败:\n{result.stderr}"


def test_lancedb_repo_connect_loads_lancedb() -> None:
    """``LanceDBManager.connect()`` 是 lancedb 的唯一启动点，须能正常加载。"""
    code = (
        "import sys, tempfile, pathlib\n"
        "from app.db.lancedb_repo import LanceDBManager\n"
        "assert 'lancedb' not in sys.modules\n"
        "with tempfile.TemporaryDirectory() as td:\n"
        "    mgr = LanceDBManager(pathlib.Path(td) / 'lancedb')\n"
        "    mgr.connect()\n"
        "    assert mgr.ensure_table('bge-large-zh-v1.5', 1, 1024)\n"
        "assert 'lancedb' in sys.modules, 'connect 后 lancedb 应已加载'\n"
    )
    result = _run_in_subprocess(code)
    assert result.returncode == 0, f"LanceDB 惰性加载失败:\n{result.stderr}"
