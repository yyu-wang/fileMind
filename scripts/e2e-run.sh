#!/usr/bin/env bash
# T9.5 E2E 运行器：每个 spec 独立 wdio 进程 + 各自全新临时数据目录，保证隔离。
#
# 为什么一 spec 一进程：wdio worker 间共享 env（FILEMIND_DATA_HOME 等），
# 若同一进程内跑多个 spec，第二个 spec 会复用第一个的 SQLite/LanceDB。
#
# 用法：
#   bash scripts/e2e-run.sh              # 本地：E2E-001/002 + spike（真实 sidecar）
#   RUN_E2E=1 bash scripts/e2e-run.sh --rag   # 含 E2E-003（需真实 Ollama 三模型）
#   bash scripts/e2e-run.sh --ci         # CI 冒烟：只跑 E2E-001/002（stub sidecar）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# —— 参数解析 ——
RUN_RAG=0
CI_MODE=0
for arg in "$@"; do
  case "$arg" in
    --rag) RUN_RAG=1 ;;
    --ci) CI_MODE=1 ;;
    *) echo "未知参数: $arg" >&2; exit 2 ;;
  esac
done

# —— 门控：E2E-003 需 RUN_E2E=1 + --rag ——
# 与 sidecar 层 E2E 一致：默认跳过，显式 RUN_E2E 才跑（需 Ollama 三模型）。
RAG_ENABLED=$([ "$RUN_RAG" = 1 ] && [ "${RUN_E2E:-0}" = 1 ] && echo 1 || echo 0)

fail_count=0
ran_any=0

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

# —— 孤儿 sidecar / 应用清理 ——
# 应用被 wdio 强杀时其子进程 sidecar 会残留并占用 SIDECAR_PORT(8765)，
# 使下一次启动无法绑定 → 握手失败退出。每个 spec 前清一次本项目的 sidecar。
# 两个模式都清：真实 PyInstaller 二进制（本地）与 CI stub（python3 sidecar_stub.py）。
# 注意：E2E 运行期间请勿同时运行真实 FileMind（两者共用同一 sidecar 端口）。
# 应用本体也要清：wdio teardown 偶发杀不掉 debug 二进制，孤儿 app 会占用
# 云端代理端口 8766，使下一个 spec 的应用启动即退出（code=1）。
_cleanup_sidecars() {
  pkill -f 'filemind-sidecar-aarch64-apple-darwin' 2>/dev/null || true
  pkill -f 'sidecar_stub.py' 2>/dev/null || true
  # 只杀本项目 debug 二进制的孤儿，不误伤正式安装的 FileMind.app
  pkill -f 'src-tauri/target/debug/filemind' 2>/dev/null || true
  sleep 1
}

for spec in e2e/specs/*.e2e.ts; do
  name="$(basename "$spec")"

  # 按 spec 编号过滤：
  #   000-spike      本地跑，CI 跳过（已由 001 覆盖应用启动断言，省一个冷启动）
  #   003-rag        仅 RAG_ENABLED 时跑
  case "$name" in
    000-*)
      [ "$CI_MODE" = 1 ] && { echo "── 跳过 ${name}（CI 精简）──"; continue; }
      ;;
    003-*)
      [ "$RAG_ENABLED" = 1 ] || { echo "── 跳过 ${name}（需 RUN_E2E=1 + --rag，真实 Ollama）──"; continue; }
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
  # loadConfig 校正后直达文件页，wizard 永不出现）；002/003 直达文件页 → SKIP。
  case "$name" in
    002-*|003-*|004-*|005-*) export FILEMIND_E2E_SKIP_ONBOARDING=1 ;;
    *)                        unset FILEMIND_E2E_SKIP_ONBOARDING ;;
  esac

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
