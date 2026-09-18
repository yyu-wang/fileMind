"""内置本地引擎（llama-server）测试的共享工具（原内联在 test_local_llm_service.py）。

非 ``test_*.py`` 命名（pytest 不收集）；供 ``test_local_llm_service``（纯逻辑 / 前置条件）
与 ``test_local_llm_service_runtime``（子进程生命周期 / 鉴权 / 孤儿清理）共用，避免两份
手抄的桩引擎脚本与临时文件准备。

``_isolate`` 夹具不在此处：本仓约定各测试文件自持该夹具（见 test_chat_retrieve_degrade.py、
chat_stream_support.py 的同款说明），避免「导入 fixture」触 ruff 的 F401/F811。

真实子进程生命周期用桩引擎（临时目录里的 Python 脚本，实现 /health）验证，仅 POSIX 可用：
Windows 上无法用 shebang 脚本当可执行文件，相关用例统一加 ``POSIX_ONLY`` 标记
（CI 的 Windows job 仍覆盖纯逻辑用例）。
"""

from __future__ import annotations

import stat
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.model_specs import LLM_MODEL_NAME, resolve_spec  # noqa: E402

#: 桩引擎依赖 shebang，Windows 不适用（纯逻辑用例不受影响）
POSIX_ONLY = pytest.mark.skipif(
    sys.platform.startswith("win"), reason="桩引擎依赖 shebang，Windows 不适用"
)

#: 桩引擎脚本：解析 --port 并实现 /health（启动即退出由环境变量 STUB_FAIL 触发）
STUB_ENGINE = '''\
#!/usr/bin/env python3
"""最小桩引擎：只为验证进程托管逻辑，不做任何推理。"""
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

if os.environ.get("STUB_FAIL"):
    # 模拟 llama.cpp 真实启动日志：带完整权重路径（用于验证错误文本脱敏）
    if os.environ.get("STUB_PATH"):
        print(f"load_model: loading model from {os.environ['STUB_PATH']}", flush=True)
    print("stub engine: failed to load model", flush=True)
    sys.exit(3)

port = int(sys.argv[sys.argv.index("--port") + 1])
expected_key = os.environ.get("LLAMA_API_KEY", "")


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler 接口
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        elif self.path == "/v1/chat/completions":
            # 与真实引擎一致：推理端点要求 Bearer key
            if expected_key and self.headers.get("Authorization") != f"Bearer {expected_key}":
                self.send_error(401)
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"choices":[{"message":{"content":"ok"}}]}')
        else:
            self.send_error(404)

    def log_message(self, *args: object) -> None:
        return


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
'''


def write_stub_engine(tmp_path: Path) -> Path:
    """写出可执行的桩引擎脚本，返回其路径。"""
    stub = tmp_path / "llama-server"
    stub.write_text(STUB_ENGINE, encoding="utf-8")
    stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
    return stub


def place_gguf(tmp_path: Path) -> Path:
    """在隔离模型目录里放一份（假）GGUF 权重，使「权重就绪」前置条件成立。"""
    gguf = resolve_spec(LLM_MODEL_NAME).root / resolve_spec(LLM_MODEL_NAME).weight
    gguf.parent.mkdir(parents=True, exist_ok=True)
    gguf.write_bytes(b"\x00" * 64)
    return gguf
