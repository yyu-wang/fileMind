#!/usr/bin/env bash
# E2E 共享模型缓存准备：把进程内 Embedding 的模型文件下到缓存目录（幂等，已就绪秒级返回）。
#
# 为什么需要它：Embedding 改为 Sidecar 进程内 ONNX 后（见 python-sidecar/app/services/
# embedding_service.py），「模型文件在本机」成为建立索引与知识问答的硬前置；而
# scripts/e2e-run.sh 给每个 spec 的都是**全新临时数据目录** → RAG spec（003）必然撞上
# EmbeddingUnavailableError。故引入一个跨 spec 复用的缓存目录，并在进 003 前准备一次。
# 006（下载 spec）刻意**不使用**本缓存，仍走全新目录真实下载，下载链路的覆盖不受影响。
#
# 为什么直接复用生产下载服务：镜像轮询、TLS 1.2 上限、3 次重试、`.part` 原子改名全在
# app.services.model_download_service 里；在这里另写一份下载逻辑，只会多出一份会漂移的真相，
# 且无法顺带验证下载器本身。
#
# 用法：
#   bash scripts/e2e-prepare-models.sh                      # 默认缓存 e2e/.cache/models
#   bash scripts/e2e-prepare-models.sh /tmp/fm-model-cache   # 指定缓存目录
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${1:-$ROOT_DIR/e2e/.cache/models}"
# 注册表默认模型（与 FILEMIND_EMBEDDING_MODEL 的默认值一致）
MODEL="bge-large-zh-v1.5"
PYTHON="$ROOT_DIR/.venv/bin/python"

if [ ! -x "$PYTHON" ]; then
  echo "❌ 未找到 Python 虚拟环境：$PYTHON（先执行 make install）" >&2
  exit 1
fi

echo "── E2E 模型缓存：${CACHE_DIR}（模型 ${MODEL}）──"
mkdir -p "$CACHE_DIR"

# cwd 切到 python-sidecar 才能 import app.*；FILEMIND_MODEL_DIR 决定下载落盘位置
cd "$ROOT_DIR/python-sidecar"
FILEMIND_MODEL_DIR="$CACHE_DIR" "$PYTHON" - "$MODEL" <<'PY'
"""确保 Embedding 模型文件就绪：已就绪直接退出，否则调生产下载服务并轮询到 ready。"""

import asyncio
import sys

from app.services import model_download_service as svc

#: 轮询间隔（秒）
POLL_SECONDS = 2.0
#: 等待上限（秒）；313MB 在慢镜像 + 3 次重试下留足余量
TIMEOUT_SECONDS = 1800.0
#: 进度打印间隔（秒）
PROGRESS_EVERY_SECONDS = 20


async def main(model: str) -> int:
    """准备模型文件。

    Args:
        model: 注册表模型名。

    Returns:
        0 表示已就绪；1 表示下载失败或等待超时。
    """
    if svc.model_ready(model):
        print("✅ 缓存已就绪，跳过下载")
        return 0

    await svc.ensure_downloaded(model)
    print("⏳ 缓存缺失，开始下载（复用生产下载服务：镜像轮询 + 3 次自动重试）")

    waited = 0.0
    while waited < TIMEOUT_SECONDS:
        await asyncio.sleep(POLL_SECONDS)
        waited += POLL_SECONDS
        status = svc.get_status(model)
        if status.status == "ready":
            megabytes = status.downloaded_bytes / 1024 / 1024
            print(f"✅ 下载完成（{megabytes:.1f} MB，镜像 {status.mirror}）")
            return 0
        if status.status == "failed":
            print(f"❌ 下载失败：{status.error}", file=sys.stderr)
            return 1
        if int(waited) % PROGRESS_EVERY_SECONDS == 0:
            megabytes = status.downloaded_bytes / 1024 / 1024
            print(
                f"   下载中 {megabytes:.1f} MB（镜像 {status.mirror}，第 {status.attempt} 次尝试）"
            )

    print(f"❌ 等待下载超时（{TIMEOUT_SECONDS:.0f}s）", file=sys.stderr)
    return 1


sys.exit(asyncio.run(main(sys.argv[1])))
PY
