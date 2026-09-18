#!/usr/bin/env bash
# T9.5 E2E 运行器：每个 spec 独立 wdio 进程 + 各自全新临时数据目录，保证隔离。
#
# 为什么一 spec 一进程：wdio worker 间共享 env（FILEMIND_DATA_HOME 等），
# 若同一进程内跑多个 spec，第二个 spec 会复用第一个的 SQLite/LanceDB。
#
# 用法：
#   bash scripts/e2e-run.sh              # 本地：E2E-001/002 + spike（真实 sidecar）
#   RUN_E2E=1 bash scripts/e2e-run.sh    # 追加 E2E-006（真实下载 313MB 模型）
#   RUN_E2E=1 bash scripts/e2e-run.sh --rag   # 追加 E2E-003（本地生成走本机真实 Ollama；
#                                             # 进程内 Embedding 的模型文件会自动准备到
#                                             # e2e/.cache/models，首次约 313MB）
#   RUN_E2E=1 bash scripts/e2e-run.sh --rag --rag-builtin
#                                            # 追加 E2E-003，但本地生成走**内置引擎**：
#                                            # 把 Ollama 指到死端口触发应用自动回落
#                                            # （T3），并准备 GGUF 权重（首次约 2GB）
#   bash scripts/e2e-run.sh --ci         # CI 冒烟：只跑 E2E-001/002（stub sidecar）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# —— 参数解析 ——
RUN_RAG=0
#: --rag-builtin：003 的本地生成改走内置 llama.cpp 引擎（验证「没装 Ollama 的机器」）
RAG_BUILTIN=0
CI_MODE=0
for arg in "$@"; do
  case "$arg" in
    --rag) RUN_RAG=1 ;;
    --rag-builtin) RUN_RAG=1; RAG_BUILTIN=1 ;;
    --ci) CI_MODE=1 ;;
    *) echo "未知参数: $arg" >&2; exit 2 ;;
  esac
done

# —— 门控：E2E-003 需 RUN_E2E=1 + --rag ——
# 与 sidecar 层 E2E 一致：默认跳过，显式 RUN_E2E 才跑（需本机真实 Ollama 生成模型；
# Embedding 已改为进程内 ONNX，模型文件由下面的共享缓存供给，不再走 Ollama）。
RAG_ENABLED=$([ "$RUN_RAG" = 1 ] && [ "${RUN_E2E:-0}" = 1 ] && echo 1 || echo 0)

# —— 门控：E2E-006 只需 RUN_E2E=1 ——
# 模型下载 spec 会真实下载 313MB（HF 镜像），默认本地 e2e 不跑，避免每次冒烟都拉一遍。
MODEL_ENABLED=$([ "${RUN_E2E:-0}" = 1 ] && echo 1 || echo 0)

# —— E2E 共享模型缓存（仅 003 等「需要已就绪模型」的 spec 使用）——
# 每个 spec 的数据目录都是全新的，而进程内 Embedding 需要模型文件已落盘，故用一份
# 跨 spec 复用的缓存，进 003 前由 scripts/e2e-prepare-models.sh 幂等准备（已就绪秒级返回）。
# 006 刻意不注入 → 仍走全新目录真实下载，下载链路覆盖不受影响。
MODEL_CACHE_DIR="${FILEMIND_E2E_MODEL_DIR:-$ROOT/e2e/.cache/models}"
#: 本地生成默认走 Ollama（Embedding/Reranker 已本地化），端口与 sidecar 层 E2E 约定一致
OLLAMA_TAGS_URL="http://127.0.0.1:11434/api/tags"
#: 内置引擎路径（--rag-builtin）用的「死端口」：让 Sidecar 判定 Ollama 不可达 → 探测
#: 自动回落到内置 llama.cpp 引擎（见 python-sidecar/app/services/inference_probe_service.py）。
#: 用 1 号端口而不是去动用户本机的 Ollama 服务：E2E 不该改宿主环境。
OLLAMA_DEAD_URL="http://127.0.0.1:1"
#: 内置引擎产物（dev 源码树；只有随包分发的 macOS arm64 / Windows x64 两平台有）
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)                  ENGINE_BIN="$ROOT/python-sidecar/vendor/llama/macos-arm64/llama-server" ;;
  MINGW*/*|MSYS*/*|CYGWIN*/*)    ENGINE_BIN="$ROOT/python-sidecar/vendor/llama/win-x64/llama-server.exe" ;;
  *)                             ENGINE_BIN="" ;;
esac

fail_count=0
ran_any=0

# —— 本地 E2E 的 Sidecar 注入：必须跑当前源码，不能落到 binaries/ 下的历史打包产物 ——
# 背景：`filemind/binaries/` 是 gitignored 的构建产物目录，本地跑过一次
# scripts/build-sidecar.sh 就会留下 onedir 产物，而 Rust 的布局发现**只认 onedir**、
# 优先级高于 dev wrapper → E2E 实际测的是那份产物里**冻结的旧 Python**。实测踩过：
# 带着修复前的 sidecar 跑了一轮 E2E，排查被带偏。
# CI 由 e2e-smoke.yml 注入 stub wrapper（FILEMIND_SIDECAR_BINARY），故这里只在未设置时兜底。
if [ -z "${FILEMIND_SIDECAR_BINARY:-}" ]; then
  VENV_PY="$ROOT/.venv/bin/python"
  if [ ! -x "$VENV_PY" ]; then
    echo "❌ 未找到 $VENV_PY：本地 E2E 需要它跑当前源码的 Sidecar（先执行 make install）" >&2
    exit 3
  fi
  # 与 filemind/binaries/filemind-sidecar-dev 等价，但临时生成——该 wrapper 是 gitignored 的，
  # 全新检出没有它，写死在脚本里会让 CI/新机器的本地 E2E 直接跑不起来。
  SIDECAR_SHIM="${TMPDIR:-/tmp}/fm-e2e-sidecar-$$.sh"
  printf '#!/bin/sh\nexec "%s" "%s"\n' "$VENV_PY" "$ROOT/python-sidecar/sidecar_entry.py" > "$SIDECAR_SHIM"
  chmod +x "$SIDECAR_SHIM"
  export FILEMIND_SIDECAR_BINARY="$SIDECAR_SHIM"
  echo "── Sidecar 注入：当前源码（$VENV_PY sidecar_entry.py）──"
fi

# —— Vite dev server 自举 ——
# 本地 E2E 用 cargo 构建的 debug 二进制（未开 custom-protocol 特性），前端页面
# 从 devUrl（http://localhost:1420，Vite 开发服务器）加载，而非内嵌 dist。
# Vite 未运行时 WKWebView 会得到错误页（opaque origin：localStorage 抛
# "The operation is insecure"、#root 为空），所有 spec 必挂。
# CI 的二进制由 `tauri build --debug` 产出（custom-protocol 开启、内嵌资源），
# 不依赖 Vite；这里拉起一个也仅是多占一个端口，无副作用。
VITE_PID=""
vite_log="${TMPDIR:-/tmp}/fm-e2e-vite-$$.log"
# lsof 按端口探测（协议无关）：Vite 只监听 IPv6 ::1，`nc -z 127.0.0.1` 会误报未就绪
_vite_ready() { lsof -iTCP:1420 -sTCP:LISTEN >/dev/null 2>&1; }
if ! _vite_ready; then
  echo "── Vite 未运行（端口 1420），自动拉起 ──"
  set -m                       # 后台任务独立进程组，结束时整组回收（含 vite 子进程）
  npm run dev >"$vite_log" 2>&1 &
  VITE_PID=$!
  set +m
  for _ in $(seq 1 60); do    # 最长等 30s
    _vite_ready && break
    sleep 0.5
  done
  if ! _vite_ready; then
    echo "❌ Vite 30s 内未就绪，日志：$vite_log"
    kill -- "-$VITE_PID" 2>/dev/null || true
    exit 1
  fi
fi
_cleanup_vite() {
  if [ -n "$VITE_PID" ]; then
    kill -- "-$VITE_PID" 2>/dev/null || true
    echo "── 已停止本次拉起的 Vite（PID ${VITE_PID}）──"
  fi
}
trap _cleanup_vite EXIT

# 临时生成的 sidecar shim 也要清（见上面「Sidecar 注入」段）
_cleanup_sidecar_shim() {
  [ -n "${SIDECAR_SHIM:-}" ] && rm -f "$SIDECAR_SHIM"
  return 0
}
trap '_cleanup_vite; _cleanup_sidecar_shim' EXIT

# —— 孤儿 sidecar / 应用清理 ——
# 应用被 wdio 强杀时其子进程 sidecar 会残留并占用 SIDECAR_PORT(8765)，
# 使下一次启动无法绑定 → 握手失败退出。每个 spec 前清一次本项目的 sidecar。
# 三个模式都清：历史打包二进制、CI stub（python3 sidecar_stub.py）与源码 wrapper
# （python sidecar_entry.py，本地默认注入的那条路径）。
# 注意：E2E 运行期间请勿同时运行真实 FileMind（两者共用同一 sidecar 端口）。
# 应用本体也要清：wdio teardown 偶发杀不掉 debug 二进制，孤儿 app 会占用
# 云端代理端口 8766，使下一个 spec 的应用启动即退出（code=1）。
_cleanup_sidecars() {
  pkill -f 'filemind-sidecar-aarch64-apple-darwin' 2>/dev/null || true
  pkill -f 'sidecar_stub.py' 2>/dev/null || true
  pkill -f 'sidecar_entry.py' 2>/dev/null || true
  # 只杀本项目 debug 二进制的孤儿，不误伤正式安装的 FileMind.app
  pkill -f 'src-tauri/target/debug/filemind' 2>/dev/null || true
  sleep 1
}

for spec in e2e/specs/*.e2e.ts; do
  name="$(basename "$spec")"

  # 按 spec 编号过滤：
  #   000-spike      本地跑，CI 跳过（已由 001 覆盖应用启动断言，省一个冷启动）
  #   003-rag        仅 RAG_ENABLED 时跑
  #   006-model      仅 MODEL_ENABLED（RUN_E2E=1）时跑（真实下载 313MB）
  case "$name" in
    000-*)
      [ "$CI_MODE" = 1 ] && { echo "── 跳过 ${name}（CI 精简）──"; continue; }
      ;;
    003-*)
      [ "$RAG_ENABLED" = 1 ] || { echo "── 跳过 ${name}（需 RUN_E2E=1 + --rag）──"; continue; }
      # 前置：本地生成后端 + 进程内 Embedding 模型。两条路径都要求「至少有一条可用的
      # 本地生成」，不满足就快速失败，不必等 spec 侧 120s 超时。
      if [ "$RAG_BUILTIN" = 1 ]; then
        # 内置引擎路径（验证「没装 Ollama 的机器」）：Ollama 指死端口 → 应用自动回落 builtin
        OLLAMA_TAGS_URL="${OLLAMA_DEAD_URL}/api/tags"
        if [ -z "$ENGINE_BIN" ]; then
          echo "❌ ${name} 前置不满足：当前平台不随包分发内置引擎（仅 macOS arm64 / Windows x64）"
          fail_count=$((fail_count + 1))
          ran_any=1
          continue
        fi
        if [ ! -x "$ENGINE_BIN" ]; then
          echo "❌ ${name} 前置不满足：内置引擎产物缺失（$ENGINE_BIN）"
          echo "   先执行 bash scripts/fetch-llama-server.sh"
          fail_count=$((fail_count + 1))
          ran_any=1
          continue
        fi
        # Embedding + GGUF 一起准备（GGUF 约 2GB，首次较慢；之后幂等秒回）
        if ! bash "$ROOT/scripts/e2e-prepare-models.sh" --with-llm "$MODEL_CACHE_DIR"; then
          echo "❌ ${name} 前置不满足：模型准备失败（Embedding / GGUF，见上方原因）"
          fail_count=$((fail_count + 1))
          ran_any=1
          continue
        fi
      else
        # 前置 1：Ollama 生成模型（默认路径）
        if ! curl -s -m 3 "$OLLAMA_TAGS_URL" >/dev/null; then
          echo "❌ ${name} 前置不满足：Ollama 不可达（$OLLAMA_TAGS_URL）"
          echo "   想不装 Ollama 跑本 spec：加 --rag-builtin（走内置引擎）"
          fail_count=$((fail_count + 1))
          ran_any=1
          continue
        fi
        # 前置 2：进程内 Embedding 的模型文件（共享缓存，幂等：已就绪秒级返回）
        if ! bash "$ROOT/scripts/e2e-prepare-models.sh" "$MODEL_CACHE_DIR"; then
          echo "❌ ${name} 前置不满足：Embedding 模型准备失败（见上方原因）"
          fail_count=$((fail_count + 1))
          ran_any=1
          continue
        fi
      fi
      ;;
    006-*)
      [ "$MODEL_ENABLED" = 1 ] || { echo "── 跳过 ${name}（需 RUN_E2E=1，真实下载 313MB 模型）──"; continue; }
      ;;
  esac

  # CI 冒烟只跑 001/002（文件头与 workflow 步骤名「E2E-001/002」的一致约束）：
  # 004/005 依赖更多交互细节，不属 lean 冒烟范围——此前本脚本漏了这道闸，
  # `--ci` 实际把 004/005 也跑了（与文档不符），在 Linux WebKitGTK 上必然失败。
  if [ "$CI_MODE" = 1 ]; then
    case "$name" in
      001-*|002-*) ;;
      *) echo "── 跳过 ${name}（CI 冒烟仅 001/002）──"; continue ;;
    esac
  fi

  # 每个 spec 全新临时目录：数据根 + 文件目录（fixture 复制进去）
  APP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fm-e2e-app-XXXXXX")"
  SCAN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fm-e2e-scan-XXXXXX")"
  case "$name" in
    003-*) cp -R e2e/fixtures/rag-docs/. "$SCAN_DIR/" ;;
    *)     cp -R e2e/fixtures/scan-tree/. "$SCAN_DIR/" ;;
  esac

  export FILEMIND_DATA_HOME="$APP_DIR"
  export FILEMIND_E2E_DATA_DIR="$SCAN_DIR"
  export FILEMIND_E2E=1
  # 000/001 测「首次引导」→ 不能 SKIP（否则 main.rs 预置 onboarding_completed=true，
  # loadConfig 校正后直达文件页，wizard 永不出现）；002~006 直达文件页 → SKIP。
  # 漏配的后果：应用停在引导向导，spec 首行等 .files-page 必挂 120s（E2E-006 踩过）。
  case "$name" in
    002-*|003-*|004-*|005-*|006-*) export FILEMIND_E2E_SKIP_ONBOARDING=1 ;;
    *)                             unset FILEMIND_E2E_SKIP_ONBOARDING ;;
  esac

  # 006 要等一次真实 313MB 下载、003 要等生成（内置引擎冷启动实测 ~22s + 长上下文预填充，
  # 两条用例各含 120s+120s 的显式等待），默认 180s 单测超时不够 → 单独上调 Mocha 超时。
  # 注意：spec 里的 `this.timeout()` 抬不动 Mocha 自己的计时器（见 006 的注释），只能走这个 env。
  case "$name" in
    006-model-download.e2e.ts) export FILEMIND_E2E_MOCHA_TIMEOUT=1800000 ;;
    003-rag-chat.e2e.ts)       export FILEMIND_E2E_MOCHA_TIMEOUT=600000 ;;
    *)                         unset FILEMIND_E2E_MOCHA_TIMEOUT ;;
  esac

  # 模型目录：只有「需要已就绪模型」的 spec 指向共享缓存（Rust 启动 Sidecar 时
  # 继承本进程 env，故这里 export 即可直达）；其余保持全新目录，让 006 继续覆盖
  # 真实下载。003 的缓存已在上面的门控分支里准备过。
  if [ "$name" = "003-rag-chat.e2e.ts" ]; then
    export FILEMIND_MODEL_DIR="$MODEL_CACHE_DIR"
  else
    unset FILEMIND_MODEL_DIR
  fi

  # 内置引擎路径（--rag-builtin）：把 Sidecar 读到的 Ollama 地址指到死端口，触发自动回落。
  # 必须逐 spec 复位：export 会泄漏给后续 spec，让它们的探测也误判 Ollama 不可达。
  # FILEMIND_E2E_LOCAL_BACKEND 告诉 spec 当前后端：内置 3B 的措辞断言要放宽（见 003 文件头）。
  if [ "$name" = "003-rag-chat.e2e.ts" ] && [ "$RAG_BUILTIN" = 1 ]; then
    export FILEMIND_OLLAMA_URL="$OLLAMA_DEAD_URL"
    export FILEMIND_E2E_LOCAL_BACKEND="builtin"
  else
    unset FILEMIND_OLLAMA_URL
    unset FILEMIND_E2E_LOCAL_BACKEND
  fi

  _cleanup_sidecars

  echo ""
  echo "════════ $name ════════"
  echo "  DATA_HOME=$APP_DIR"
  echo "  SCAN_DIR =$SCAN_DIR"
  if npx wdio run e2e/wdio.conf.ts --spec "$spec"; then
    echo "✅ $name 通过"
  else
    echo "❌ $name 失败"
    fail_count=$((fail_count + 1))
  fi
  ran_any=1

  # 分类产物落在扫描根同级的收纳根 `<扫描根名>_已分类`，一并清理
  rm -rf "$APP_DIR" "$SCAN_DIR" "${SCAN_DIR}_已分类"
done

echo ""
if [ "$ran_any" = 0 ]; then
  echo "⚠️  没有可运行的 spec（检查参数与门控）"
  exit 2
fi
if [ "$fail_count" -gt 0 ]; then
  echo "E2E 结束：$fail_count 个 spec 失败"
  exit 1
fi
echo "E2E 全部通过"
