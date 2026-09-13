#!/usr/bin/env bash
# T4 / P2-2 打包态结构验证脚本：检查安装包（macOS .app 目录）内 Sidecar 集成是否完整。
#
# 覆盖 T3 smoke 之外的打包态自动可测项（结构层）：
#   1. Sidecar onedir 目录存在于 $RESOURCE/sidecar/（P2-2 起经 bundle.resources 落地）
#   2. 主可执行位 + 目录总体积 ≤1600MB（与 scripts/build-sidecar.sh 门控一致）
#      注：本脚本量的是**打包后**目录（约 1384MB，Tauri 已把 36 个 symlink 解引用成真实文件），
#      build-sidecar.sh 量的是**源产物**（915MB）。两者共用同一门控。
#   3. 主可执行文件头架构与当前机器一致（Mach-O 的 arm64/x86_64）
#
# 运行期功能项（启动/握手/崩溃重启/内存）见 scripts/go-no-go.py —— 属于发布前
# 人工清单（docs/packaging-implementation-plan.md T4/T6），需在目标机器 + 应用
# 真机交互下执行，本脚本不做。
#
# 用法：
#   bash scripts/verify-packaged-app.sh <FileMind.app 目录>
# 退出码：0 = 全部通过；1 = 任一失败
set -euo pipefail

APP="${1:-}"
if [[ -z "${APP}" || ! -d "${APP}" ]]; then
  echo "[FAIL] 用法: $0 <FileMind.app 目录>" >&2
  exit 2
fi
APP="$(cd "$APP" && pwd)"

fail() { echo "[FAIL] $*" >&2; FAILED=1; }
PASS_COUNT=0
FAILED=0

# 统计目录内所有文件的逻辑大小之和（MB）。
# 不用 `du`：APFS clone/硬链接会让同一棵树在不同副本间给出不一致的块占用（929MB vs 1397MB 实测）。
tree_size_mb() {
  local dir="$1" bytes
  if [[ "$(uname -s)" == "Darwin" ]]; then
    bytes="$(find "${dir}" -type f -exec stat -f %z {} + | awk '{s+=$1} END {print s+0}')"
  else
    bytes="$(find "${dir}" -type f -exec stat -c %s {} + | awk '{s+=$1} END {print s+0}')"
  fi
  echo $(( bytes / 1024 / 1024 ))
}

# ---- 1. Sidecar onedir 目录（$RESOURCE/sidecar/） ----
# P2-2：tauri.conf.json `bundle.resources` 的 map 目标为 "sidecar"，
# 故主可执行在 Contents/Resources/sidecar/filemind-sidecar。
SIDECAR_DIR="${APP}/Contents/Resources/sidecar"
SIDECAR=""
if [[ -d "${SIDECAR_DIR}" ]]; then
  for cand in \
    "${SIDECAR_DIR}/filemind-sidecar" \
    "${SIDECAR_DIR}/filemind-sidecar-aarch64-apple-darwin" \
    "${SIDECAR_DIR}/filemind-sidecar-x86_64-apple-darwin"; do
    if [[ -f "${cand}" ]]; then SIDECAR="${cand}"; break; fi
  done
fi
if [[ -n "${SIDECAR}" ]]; then
  echo "[PASS] Sidecar onedir 已集成: ${SIDECAR}"
  PASS_COUNT=$((PASS_COUNT + 1))
  if [[ -d "${SIDECAR_DIR}/_internal" ]]; then
    echo "[PASS] Sidecar _internal/ 已集成（onedir 运行时依赖）"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    fail "_internal/ 缺失：onedir 产物不完整，启动会失败"
  fi
else
  fail "Contents/Resources/sidecar/ 下未找到 filemind-sidecar（bundle.resources 未打进包）"
fi

# ---- 2. 可执行位 + 体积门控 ≤1200MB（按目录解压态统计） ----
if [[ -n "${SIDECAR}" ]]; then
  if [[ ! -x "${SIDECAR}" ]]; then
    fail "Sidecar 主可执行无可执行权限"
  else
    SIZE_MB=$(tree_size_mb "${SIDECAR_DIR}")
    if (( SIZE_MB > 1600 )); then
      fail "Sidecar 目录体积 ${SIZE_MB}MB > 1600MB 门控"
    else
      echo "[PASS] Sidecar 目录体积 ${SIZE_MB}MB ≤ 1600MB"
      PASS_COUNT=$((PASS_COUNT + 1))
    fi
  fi
fi

# ---- 3. Mach-O 架构与当前机器一致 ----
if [[ -n "${SIDECAR}" && "$(uname -s)" == "Darwin" ]]; then
  HOST_ARCH="$(uname -m)"
  FILE_ARCH="$(file -b "${SIDECAR}" | sed -E 's/.*(arm64|x86_64).*/\1/')"
  if [[ "${FILE_ARCH}" == "${HOST_ARCH}" ]]; then
    echo "[PASS] Sidecar 架构 ${FILE_ARCH} 匹配当前机器 ${HOST_ARCH}"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    fail "Sidecar 架构 ${FILE_ARCH} ≠ 当前机器 ${HOST_ARCH}"
  fi
fi

echo "--- T4 结构验证结果: ${PASS_COUNT} PASS / $(if [[ ${FAILED} -eq 1 ]]; then echo 'FAIL'; else echo '0 FAIL'; fi) ---"
exit "${FAILED}"
