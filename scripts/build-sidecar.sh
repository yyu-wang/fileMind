#!/usr/bin/env bash
# T1.2 / P2-2 PyInstaller 构建脚本：产出 FileMind Sidecar 三平台 onedir 可执行目录。
#
# P2-2（2026-09-11）：由 onefile 改为 onedir——onefile 每次启动需把 ~320MB 归档解压到
# 临时目录（实测冷启动 21s / 热盘 16s 中约 14s 花在解压），onedir 原地读取消除该开销。
#
# 用法:
#   bash scripts/build-sidecar.sh                    # 自动识别本机架构（mac arm64 常见）
#   bash scripts/build-sidecar.sh --target aarch64-apple-darwin
#   bash scripts/build-sidecar.sh --target x86_64-apple-darwin --no-clean
#   bash scripts/build-sidecar.sh --list-targets      # 打印支持 triple
#
# DoD（T1.2 验收 + 2026-09-04 门控修订 + P2-2 形态变更）:
#   1. 三平台各自产生 <filemind>/binaries/filemind-sidecar-{triple}/ 目录，
#      内含主可执行 filemind-sidecar[.exe] 与 _internal/
#   2. 每个目录总体积 ≤1600MB（脚本 assert，超标 exit 5）。
#      ⚠️ 门控口径变更（P2-2 2026-09-11）：onefile 时代门控 400MB 量的是**压缩态**
#      单文件（319MB）；onedir 是**解压落地态**目录，故改按解压态绝对值设门控。
#      实测（本机 aarch64）：源产物 915MB / 6136 文件（保留 PyInstaller 的 symlink），
#      用户实际下载体积 tar.gz 317MB（与原 onefile 319MB 持平），故门控取 1600MB。
#      ⚠️ 打包后体积更大（约 1384MB / 6172 文件）：Tauri `copy_resources` 会把源产物里的
#      symlink（Python.framework 的 Versions/Current 等 36 个）**解引用**成真实文件副本。
#      本脚本量的是**源产物**（915MB），scripts/verify-packaged-app.sh 量的是**打包后**
#      （1384MB），两者共用 1600MB 门控，均 PASS。
#      ⚠️ 体积一律按**逻辑大小**（`find -type f` 逐个 size 求和，不跟随 symlink）统计，
#      不用 `du`：APFS 上 clone/硬链接共享块会让同一棵树的不同副本给出不一致读数。
#   3. macOS 本机额外软链接 filemind/binaries/filemind-sidecar → 当前架构产物目录，
#      供 src-tauri manager.rs 默认路径直接使用
#
# 启动性能（本机 aarch64 实测 2026-09-11）：onedir 稳态 /health 就绪 **1.0s**
# （onefile 为 15.7s）；首次启动（冷页面缓存需读满 ~1.4GB）约 15s，后续均 1s。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYSIDE_DIR="$ROOT_DIR/python-sidecar"
BINARIES_DIR="$ROOT_DIR/filemind/binaries"
# 体积门控（MB，逻辑大小）：2026-09-04 由 80 上调至 400（onefile 压缩态口径）；
# P2-2 改为 onedir 后按解压态实测绝对值上调至 1600，理由见文件头 DoD 注释。
MAX_SIZE_MB=1600

# 统计目录内所有文件的逻辑大小之和（MB）。
# 不用 `du`：APFS clone/硬链接会让同一棵树在不同副本间给出不一致的块占用
# （实测同一产物 929MB vs 1397MB），无法作为跨副本可比的门控口径。
tree_size_mb() {
  local dir="$1" bytes
  if [[ "$(uname -s)" == "Darwin" ]]; then
    bytes="$(find "${dir}" -type f -exec stat -f %z {} + | awk '{s+=$1} END {print s+0}')"
  else
    bytes="$(find "${dir}" -type f -exec stat -c %s {} + | awk '{s+=$1} END {print s+0}')"
  fi
  echo $(( bytes / 1024 / 1024 ))
}
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
  # Windows venv 布局（Scripts/python.exe）；CI windows runner 用
  if [[ -x "${ROOT_DIR}/.venv/Scripts/python.exe" ]]; then
    PYEXE="${ROOT_DIR}/.venv/Scripts/python.exe"
  fi
fi
if [[ ! -x "${PYEXE}" ]]; then
  PYEXE="$(command -v python3 || command -v python || true)"
  if [[ -z "${PYEXE}" ]]; then
    echo "[FATAL] 找不到 python: 既无 <repo>/.venv 也无 PATH python/python3" >&2
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

# ---------- 产物检查（onedir：dist/filemind-sidecar/ 目录） ----------
SRC_DIR="dist/filemind-sidecar"
SRC="${SRC_DIR}/filemind-sidecar${EXE_EXT}"
if [[ ! -d "${SRC_DIR}" || ! -f "${SRC}" ]]; then
  echo "[FATAL] 期望产物不存在: ${PYSIDE_DIR}/${SRC_DIR}/（需含主可执行 filemind-sidecar${EXE_EXT}）" >&2
  exit 4
fi
SIZE_MB="$(tree_size_mb "${SRC_DIR}")"
FILE_COUNT="$(find "${SRC_DIR}" -type f | wc -l | tr -d ' ')"

# 目录产物指纹：按相对路径排序逐文件 sha256 → 汇总再 sha256。
# 确定性等价于原单文件 sha256 语义（onedir 无单一产物文件可哈希）。
# Windows Git Bash 无 shasum（Perl 脚本），回退 sha256sum（coreutils 自带）。
if command -v shasum >/dev/null 2>&1; then
  _sha_file() { shasum -a 256 "$1" | awk '{print $1}'; }
  _sha_stdin() { shasum -a 256 | awk '{print $1}'; }
else
  _sha_file() { sha256sum "$1" | awk '{print $1}'; }
  _sha_stdin() { sha256sum | awk '{print $1}'; }
fi
SHA="$( (cd "${SRC_DIR}" && find . -type f | LC_ALL=C sort | while IFS= read -r f; do _sha_file "$f"; done) | _sha_stdin )"

echo "[build] 产物: ${SRC_DIR}/"
echo "           size: ${SIZE_MB}MB (${FILE_COUNT} files)"
echo "           sha256: ${SHA}"

# 体积断言：T1.2 DoD 硬约束（门控 2026-09-04 修订为 400MB，按目录总大小，见文件头注释）
if (( SIZE_MB >= MAX_SIZE_MB )); then
  cat <<EOF
[FAIL] 体积 ${SIZE_MB}MB ≥ 门控 ${MAX_SIZE_MB}MB（T1.2 DoD 未通过）。

建议：
  1) 检查是否误收非必要数据（--excludes / collect_data_files 过滤）
  2) 若仍超标，重新评估依赖集或门控（当前已是 onedir，无解压开销可再优化）
EOF
  exit 5
fi
echo "[build] 体积 PASS (${SIZE_MB}MB < ${MAX_SIZE_MB}MB)"

# ---------- 复制到 binaries/（整个 onedir 目录） ----------
DST_NAME="filemind-sidecar-${TARGET}"
DST_DIR="${BINARIES_DIR}/${DST_NAME}"
if [[ ${NO_COPY} -eq 0 ]]; then
  # 先删旧目录：残留的 _internal 旧文件会与新版混用导致诡异 ImportError
  rm -rf "${DST_DIR}"
  cp -R "${SRC_DIR}" "${DST_DIR}"
  chmod +x "${DST_DIR}/filemind-sidecar${EXE_EXT}"
  echo "[build] 复制 → ${DST_DIR}/"
  # macOS 本机构建：默认名软链接（指向目录），供 manager.rs 默认路径直接使用
  case "${TARGET}" in
    aarch64-apple-darwin|x86_64-apple-darwin)
      DEF_LINK="${BINARIES_DIR}/filemind-sidecar"
      # 清掉旧链接或旧目录：`rm -f` 遇同名真实目录会报错，配合 set -e 直接中断
      # （实测过：手工把产物放在默认名下再跑构建即触发）。真实产物已在
      # ${DST_DIR}，故此处删除默认名下的旧目录是安全的。
      if [[ -d "${DEF_LINK}" && ! -L "${DEF_LINK}" ]]; then
        rm -rf "${DEF_LINK}"
      else
        rm -f "${DEF_LINK}"
      fi
      ln -s "${DST_NAME}" "${DEF_LINK}"
      echo "[build] 默认路径软链接 → ${DEF_LINK} -> ${DST_NAME}"
      ;;
  esac
fi

echo "--- T1.2 构建报告 ---"
printf 'Target:    %s\n' "${TARGET}"
printf 'Arch(PI):  %s\n' "${ARCH_PYI}"
printf 'Size(MB):  %s\n' "${SIZE_MB}"
printf 'Files:     %s\n' "${FILE_COUNT}"
printf 'SHA-256:   %s\n' "${SHA}"
printf 'Output:    %s/\n' "${DST_DIR}"
echo "[build] OK ✓"
