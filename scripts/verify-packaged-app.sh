#!/usr/bin/env bash
# T4 打包态结构验证脚本：检查安装包（macOS .app 目录）内 Sidecar 集成是否完整。
#
# 覆盖 T3 smoke 之外的打包态自动可测项（结构层）：
#   1. Sidecar 可执行文件存在于主可执行同目录（Contents/MacOS/filemind-sidecar）
#   2. 可执行位 + 体积 ≤ 400MB（与 scripts/build-sidecar.sh 门控一致）
#   3. 文件头架构与当前机器一致（Mach-O 的 arm64/x86_64）
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

# ---- 1. Sidecar 在主可执行同目录 ----
SIDECAR=""
if [[ -d "${APP}/Contents/MacOS" ]]; then
  for cand in "${APP}/Contents/MacOS/filemind-sidecar" "${APP}/Contents/MacOS/filemind-sidecar-aarch64-apple-darwin" "${APP}/Contents/MacOS/filemind-sidecar-x86_64-apple-darwin"; do
    if [[ -f "${cand}" ]]; then SIDECAR="${cand}"; break; fi
  done
fi
if [[ -n "${SIDECAR}" ]]; then
  echo "[PASS] Sidecar 已集成于主可执行同目录: ${SIDECAR}"
  PASS_COUNT=$((PASS_COUNT + 1))
else
  fail "Contents/MacOS 下未找到 filemind-sidecar（externalBin 未打进包）"
fi

# ---- 2. 可执行位 + 体积门控 ≤400MB ----
if [[ -n "${SIDECAR}" ]]; then
  if [[ ! -x "${SIDECAR}" ]]; then
    fail "Sidecar 无可执行权限"
  else
    SIZE_MB=$(du -m "${SIDECAR}" | awk '{print $1}')
    if (( SIZE_MB > 400 )); then
      fail "Sidecar 体积 ${SIZE_MB}MB > 400MB 门控"
    else
      echo "[PASS] Sidecar 体积 ${SIZE_MB}MB ≤ 400MB"
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
