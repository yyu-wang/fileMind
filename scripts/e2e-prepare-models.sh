#!/usr/bin/env bash
# E2E 共享模型缓存准备：把 E2E 需要的模型文件下到缓存目录（幂等，已就绪秒级返回）。
#
# 为什么需要它：Embedding 改为 Sidecar 进程内 ONNX 后（见 python-sidecar/app/services/
# embedding_service.py），「模型文件在本机」成为建立索引与知识问答的硬前置；而
# scripts/e2e-run.sh 给每个 spec 的都是**全新临时数据目录** → RAG spec（003）必然撞上
# EmbeddingUnavailableError。故引入一个跨 spec 复用的缓存目录，并在进 003 前准备一次。
# 006（下载 spec）刻意**不使用**本缓存，仍走全新目录真实下载，下载链路的覆盖不受影响。
#
# T3 起新增 `--with-llm`：内置 llama.cpp 引擎的 GGUF 权重（约 2GB）也走同一份缓存——
# 「没装 Ollama 的机器」那条路径（`e2e-run.sh --rag --rag-builtin`）需要它；不带该参数
# 时只准备 Embedding，不给日常 E2E 加 2GB 下载。
#
# 为什么直接复用生产下载服务：镜像轮询、TLS 1.2 上限、3 次重试、`.part` 原子改名全在
# app.services.model_download_service 里；在这里另写一份下载逻辑，只会多出一份会漂移的真相，
# 且无法顺带验证下载器本身。
#
# 用法：
#   bash scripts/e2e-prepare-models.sh                      # 默认缓存 e2e/.cache/models
#   bash scripts/e2e-prepare-models.sh --with-llm           # 追加 GGUF（约 2GB，一次性）
#   bash scripts/e2e-prepare-models.sh /tmp/fm-model-cache  # 指定缓存目录
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${ROOT_DIR}/e2e/.cache/models"
#: 是否额外准备 GGUF（内置引擎权重）
WITH_LLM=0

usage() {
  cat <<'EOF'
Usage: e2e-prepare-models.sh [--with-llm] [CACHE_DIR]

Options:
  --with-llm      额外准备内置引擎的 GGUF 权重（qwen2.5-3b-instruct，约 2GB）
  CACHE_DIR       缓存目录（默认 <repo>/e2e/.cache/models）
EOF
}

for arg in "$@"; do
  case "$arg" in
    --with-llm) WITH_LLM=1 ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "未知参数: $arg" >&2; usage >&2; exit 2 ;;
    *) CACHE_DIR="$arg" ;;
  esac
done

# 注册表默认模型（与 FILEMIND_EMBEDDING_MODEL 的默认值一致）
MODELS=("bge-large-zh-v1.5")
if [[ "${WITH_LLM}" -eq 1 ]]; then
  MODELS+=("qwen2.5-3b-instruct")
fi

PYTHON="$ROOT_DIR/.venv/bin/python"

if [ ! -x "$PYTHON" ]; then
  echo "❌ 未找到 Python 虚拟环境：$PYTHON（先执行 make install）" >&2
  exit 1
fi

echo "── E2E 模型缓存：${CACHE_DIR}（模型：${MODELS[*]}）──"
mkdir -p "$CACHE_DIR"

# cwd 切到 python-sidecar 才能 import app.*；FILEMIND_MODEL_DIR 决定下载落盘位置
cd "$ROOT_DIR/python-sidecar"
FILEMIND_MODEL_DIR="$CACHE_DIR" "$PYTHON" - "${MODELS[@]}" <<'PY'
"""确保 E2E 需要的模型文件就绪：已就绪直接跳过，否则调生产下载服务并轮询到 ready。"""

import asyncio
import sys

from app.services import model_download_service as svc

#: 轮询间隔（秒）
POLL_SECONDS = 2.0
#: 进度打印间隔（秒）
PROGRESS_EVERY_SECONDS = 20
#: 等待上限（秒）：313MB 的 Embedding 在慢镜像 + 3 次重试下留足余量
DEFAULT_TIMEOUT_SECONDS = 1800.0
#: 个别更大的模型单独给上限（GGUF 约 2GB，慢镜像按 1MB/s 估算需 ~34min）
TIMEOUT_SECONDS: dict[str, float] = {"qwen2.5-3b-instruct": 7200.0}


async def prepare(model: str) -> int:
    """准备单个模型文件（幂等）。

    Args:
        model: 注册表模型名（Embedding / Rerank / GGUF）。

    Returns:
        0 表示已就绪；1 表示下载失败或等待超时。
    """
    if svc.model_ready(model):
        print(f"✅ {model} 已就绪，跳过下载")
        return 0

    await svc.ensure_downloaded(model)
    print(f"⏳ {model} 缺失，开始下载（复用生产下载服务：镜像轮询 + 3 次自动重试）")

    timeout = TIMEOUT_SECONDS.get(model, DEFAULT_TIMEOUT_SECONDS)
    waited = 0.0
    while waited < timeout:
        await asyncio.sleep(POLL_SECONDS)
        waited += POLL_SECONDS
        status = svc.get_status(model)
        if status.status == "ready":
            megabytes = status.downloaded_bytes / 1024 / 1024
            print(f"✅ {model} 下载完成（{megabytes:.1f} MB，镜像 {status.mirror}）")
            return 0
        if status.status == "failed":
            print(f"❌ {model} 下载失败：{status.error}", file=sys.stderr)
            return 1
        if int(waited) % PROGRESS_EVERY_SECONDS == 0:
            megabytes = status.downloaded_bytes / 1024 / 1024
            print(
                f"   {model} 下载中 {megabytes:.1f} MB"
                f"（镜像 {status.mirror}，第 {status.attempt} 次尝试）"
            )

    print(f"❌ {model} 等待下载超时（{timeout:.0f}s）", file=sys.stderr)
    return 1


async def main(models: list[str]) -> int:
    """按序准备全部模型（任一失败即整体失败，但不跳过后面的模型）。

    Args:
        models: 待准备的模型名列表。

    Returns:
        0 表示全部就绪；1 表示有模型失败或超时。
    """
    codes = [await prepare(model) for model in models]
    return max(codes, default=0)


sys.exit(asyncio.run(main(sys.argv[1:])))
PY
