#!/usr/bin/env bash
# T1.2 PyInstaller 构建脚本：产出 FileMind Sidecar 三平台 onefile 可执行文件。
#
# 用法:
#   bash scripts/build-sidecar.sh                    # 自动识别本机架构（mac arm64 常见）
#   bash scripts/build-sidecar.sh --target aarch64-apple-darwin
#   bash scripts/build-sidecar.sh --target x86_64-apple-darwin --no-clean
#   bash scripts/build-sidecar.sh --list-targets      # 打印支持 triple
#
# DoD（T1.2 验收）:
#   1. 三平台各自产生 <filemind>/binaries/filemind-sidecar-{triple}[.exe]
#   2. 每个文件体积 <80MB（脚本 assert，超标 exit 1，建议再加 --exclude）
#   3. macOS 本机额外软链接 filemind/binaries/filemind-sidecar → 当前架构产物，
#      供 src-tauri manager.rs 默认路径直接使用
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYSIDE_DIR="$ROOT_DIR/python-sidecar"
BINARIES_DIR="$ROOT_DIR/filemind/binaries"
MAX_SIZE_MB=80
TARGET=""
NO_CLEAN=0
NO_COPY=0
LIST_TARGETS=0

usage() {
  cat <<'EOF'
Usage: build-sidecar.sh [OPTIONS]

Options:
  --target TRIPLE      Target triple:
                         aarch64-apple-darwin
                         x86_64-apple-darwin
                         x86_64-pc-windows-msvc
                         x86_64-unknown-linux-gnu
                       (默认按本机自动)
  --list-targets       打印支持的 triples
  --no-clean           不删 python-sidecar/{build,dist} 上次产物（加速二次 build）
  --no-copy            构建完不复制到 binaries/（调试 spec 用）
  -h, --help           显示本帮助
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --list-targets) LIST_TARGETS=1; shift ;;
    --no-clean) NO_CLEAN=1; shift ;;
    --no-copy) NO_COPY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown: $1" >&2; usage >&2; exit 2 ;;
  esac
done

SUPPORTED=(aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu)

if [[ $LIST_TARGETS -eq 1 ]]; then
  for t in "${SUPPORTED[@]}"; do echo "$t"; done
  exit 0
fi

# ---------- auto detect triple ----------
if [[ -z "${TARGET}" ]]; then
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "${OS}-${ARCH}" in
    darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
    darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
    linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
    msys*-x86_64|mingw*-x86_64|cygwin*-x86_64) TARGET="x86_64-pc-windows-msvc" ;;
    *) echo "[ERROR] 无法识别本机 (OS=${OS}, ARCH=${ARCH})，请 --target 指定"; exit 2 ;;
  esac
  echo "[build] 自动识别 target: ${TARGET}"
fi

# ---------- triple 校验 + arch/os/exe_ext 解析 ----------
VALID=0
ARCH_PYI=""
EXE_EXT=""
for t in "${SUPPORTED[@]}"; do
  if [[ "${t}" == "${TARGET}" ]]; then VALID=1; fi
done
if [[ $VALID -ne 1 ]]; then
  echo "[ERROR] target ${TARGET} 不在支持列表:" >&2; printf '  - %s\n' "${SUPPORTED[@]}" >&2; exit 2
fi
case "${TARGET}" in
  aarch64-apple-darwin)        ARCH_PYI="arm64"   ; EXE_EXT=""   ;;
  x86_64-apple-darwin)         ARCH_PYI="x86_64"  ; EXE_EXT=""   ;;
  x86_64-pc-windows-msvc)      ARCH_PYI="x86_64"  ; EXE_EXT=".exe" ;;
  x86_64-unknown-linux-gnu)    ARCH_PYI="x86_64"  ; EXE_EXT=""   ;;
esac

# ---------- 环境：venv + pyinstaller ----------
PYEXE="${ROOT_DIR}/.venv/bin/python"
if [[ ! -x "${PYEXE}" ]]; then
  PYEXE="$(command -v python3 || true)"
  if [[ -z "${PYEXE}" ]]; then
    echo "[FATAL] 找不到 python: 既无 <repo>/.venv/bin/python 也无 PATH python3" >&2
    exit 3
  fi
fi
"${PYEXE}" -m PyInstaller --version >/dev/null 2>&1 || {
  echo "[FATAL] 未装 PyInstaller：请 pip install -r python-sidecar/requirements-dev.txt" >&2
  exit 3
}

mkdir -p "${BINARIES_DIR}"
echo "[build] 进入 python-sidecar"
cd "${PYSIDE_DIR}"

if [[ ${NO_CLEAN} -eq 0 ]]; then
  echo "[build] 清理 build/ dist/"
  rm -rf build dist
fi

# ---------- PyInstaller ----------
echo "[build] PyInstaller (arch=${ARCH_PYI}, spec=filemind-sidecar.spec)"
# --target-arch 参数只对 .py 目标生效；.spec 场景通过环境变量让 spec 内部
# 的 EXE(target_arch=...) 取到正确值（PyInstaller 6.x 官方文档推荐方式）。
#
# 缓存目录 PYINSTALLER_CONFIG_DIR 放在项目内 .cache/pyinstaller：
#   - 避免默认路径 ``~/Library/Application Support/pyinstaller`` 可能的权限报错
#   - 便于在 .gitignore 中整体忽略
export PYINSTALLER_RUNTIME=1
export PYINSTALLER_TARGET_ARCH="${ARCH_PYI}"
export PYINSTALLER_CONFIG_DIR="${PYSIDE_DIR}/.cache/pyinstaller"
mkdir -p "${PYINSTALLER_CONFIG_DIR}"
"${PYEXE}" -m PyInstaller \
  --clean \
  --noconfirm \
  filemind-sidecar.spec

# ---------- 产物检查 ----------
SRC="dist/filemind-sidecar${EXE_EXT}"
if [[ ! -f "${SRC}" ]]; then
  echo "[FATAL] 期望产物不存在: ${PYSIDE_DIR}/${SRC}" >&2
  exit 4
fi
SIZE_MB="$(du -m "${SRC}" | awk '{print $1}')"
SHA="$(shasum -a 256 "${SRC}" | awk '{print $1}')"
echo "[build] 产物: ${SRC}"
echo "           size: ${SIZE_MB}MB"
echo "           sha256: ${SHA}"

# 体积断言：T1.2 DoD 硬约束
if (( SIZE_MB >= MAX_SIZE_MB )); then
  cat <<EOF
[FAIL] 体积 ${SIZE_MB}MB ≥ 目标 ${MAX_SIZE_MB}MB（T1.2 DoD 未通过）。

建议：
  1) 在 filemind-sidecar.spec excludes 列表加更多不常用 stdlib / numpy 子包
  2) 尝试 upx=True（注意 native .so 兼容性）
  3) 评估拆分成 sidecar_heavy.py 懒加载模块，进一步 exclude
EOF
  exit 5
fi
echo "[build] 体积 PASS (${SIZE_MB}MB < ${MAX_SIZE_MB}MB)"

# ---------- 复制到 binaries/ ----------
DST_NAME="filemind-sidecar-${TARGET}${EXE_EXT}"
DST="${BINARIES_DIR}/${DST_NAME}"
if [[ ${NO_COPY} -eq 0 ]]; then
  cp -f "${SRC}" "${DST}"
  chmod +x "${DST}"
  echo "[build] 复制 → ${DST}"
  # macOS 本机构建：默认名软链接，供 manager.rs 默认路径直接使用
  case "${TARGET}" in
    aarch64-apple-darwin|x86_64-apple-darwin)
      DEF_LINK="${BINARIES_DIR}/filemind-sidecar"
      rm -f "${DEF_LINK}"
      ln -s "${DST_NAME}" "${DEF_LINK}"
      echo "[build] 默认路径软链接 → ${DEF_LINK} -> ${DST_NAME}"
      ;;
  esac
fi

echo "--- T1.2 构建报告 ---"
printf 'Target:    %s\n' "${TARGET}"
printf 'Arch(PI):  %s\n' "${ARCH_PYI}"
printf 'Size(MB):  %s\n' "${SIZE_MB}"
printf 'SHA-256:   %s\n' "${SHA}"
printf 'Output:    %s\n' "${DST}"
echo "[build] OK ✓"
