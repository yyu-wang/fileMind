#!/usr/bin/env bash
# T3c — 取「内置 llama.cpp 引擎」产物到 python-sidecar/vendor/llama/<platform>/
#
# 为什么需要它：T3 的目标是「未安装 Ollama 的部署机器也能知识问答」，靠的是随安装包
# 分发的**预编译 llama-server 二进制**（Sidecar 在需要时把它作为子进程拉起）。
# 产物不入库（macOS 解压后约 27MB；Windows 约 30MB），构建前必须现取，故
# scripts/build-sidecar.sh 会在 PyInstaller 之前自动调用本脚本。
#
# 为什么下载用 Python 而不是 curl：
#   1. GitHub release 的 CDN 与 TLS 1.3 组合会稳定触发 SSLV3_ALERT_BAD_RECORD_MAC
#      （与模型下载同源，解法也是同一个：TLS 上限压到 1.2）；下载偶发 UNEXPECTED_EOF，
#      需要重试；
#   2. 实测部分开发机 curl 直连 GitHub 完全不通（HTTP 000），而 httpx（走系统代理）正常。
#
# 用法:
#   bash scripts/fetch-llama-server.sh                            # 按本机平台
#   bash scripts/fetch-llama-server.sh --target x86_64-pc-windows-msvc
#   bash scripts/fetch-llama-server.sh --force                    # 忽略已就绪产物，重新下载
#
# 未支持平台（Linux / macOS x64）：打印警告后**正常退出 0**，不阻断构建——这两个平台
# 不对外分发（见 merge-build.yml 的平台矩阵），Sidecar 运行时会如实报「引擎可执行文件缺失」。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=""
FORCE=0

usage() {
  cat <<'EOF'
Usage: fetch-llama-server.sh [OPTIONS]

Options:
  --target TRIPLE   按目标平台取产物（默认按本机）:
                      aarch64-apple-darwin      → macos-arm64
                      x86_64-pc-windows-msvc    → win-x64
                      x86_64-apple-darwin       → 未支持（跳过）
                      x86_64-unknown-linux-gnu  → 未支持（跳过）
  --force           忽略已有的就绪产物，重新下载
  -h, --help        显示本帮助
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --force) FORCE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# ---------- 目标 triple → 平台键（必须与 app/services/local_llm_service.platform_key() 一致） ----------
if [[ -z "${TARGET}" ]]; then
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "${OS}-${ARCH}" in
    darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
    darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
    msys*-x86_64|mingw*-x86_64|cygwin*-x86_64) TARGET="x86_64-pc-windows-msvc" ;;
    linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
    *) echo "[engine] 无法识别本机 (OS=${OS}, ARCH=${ARCH})，请 --target 指定" >&2; exit 2 ;;
  esac
fi

case "${TARGET}" in
  aarch64-apple-darwin)     PLATFORM_KEY="macos-arm64" ;;
  x86_64-pc-windows-msvc)   PLATFORM_KEY="win-x64" ;;
  *)
    echo "[engine] 平台 ${TARGET} 不随包分发内置引擎，跳过下载（Sidecar 运行时会如实报缺失）"
    exit 0
    ;;
esac

# ---------- Python 解释器（venv 优先，CI Windows runner 用 Scripts/） ----------
PYTHON="$ROOT_DIR/.venv/bin/python"
if [[ ! -x "${PYTHON}" && -x "$ROOT_DIR/.venv/Scripts/python.exe" ]]; then
  PYTHON="$ROOT_DIR/.venv/Scripts/python.exe"
fi
if [[ ! -x "${PYTHON}" ]]; then
  PYTHON="$(command -v python3 || command -v python || true)"
fi
if [[ -z "${PYTHON}" ]]; then
  echo "[engine] [FATAL] 找不到 python（先执行 make install）" >&2
  exit 3
fi

VENDOR_DIR="$ROOT_DIR/python-sidecar/vendor/llama"
mkdir -p "${VENDOR_DIR}"

echo "[engine] 目标平台 ${PLATFORM_KEY}（triple=${TARGET}）→ ${VENDOR_DIR}/${PLATFORM_KEY}/"

"${PYTHON}" - "${PLATFORM_KEY}" "${VENDOR_DIR}" "${FORCE}" <<'PY'
"""下载并校验 llama.cpp 的 llama-server 产物（pin 版本 + sha256）。"""

import hashlib
import io
import os
import shutil
import ssl
import sys
import tarfile
import time
import zipfile
from pathlib import Path

import httpx

#: 各平台 pin 死的产物（**绝不用 latest/动态解析 tag**：引擎随安装包分发，构建必须可复现。
#: sha256 取自 GitHub release assets API 的 digest 字段，下载后再本地复核一次。）
#:   key → (tag, asset 文件名, sha256, 归档类型)
ASSETS: dict[str, tuple[str, str, str, str]] = {
    "macos-arm64": (
        "b10900",
        "llama-b10900-bin-macos-arm64.tar.gz",
        "55653efb98c90b39d26cfea403f9a270d364fcba8c4714006e82a1b0265948d0",
        "tar.gz",
    ),
    "win-x64": (
        "b10900",
        "llama-b10900-bin-win-cpu-x64.zip",
        "47f6fe584bc8a6510c00f40ed103ec43db66a52f4eee23126b8379c9028fd107",
        "zip",
    ),
}

#: 下载重试次数与退避（秒）。GitHub CDN 实测偶发两类失败：连接超时（ConnectTimeout）
#: 与传输中被打断（RemoteProtocolError, received N bytes），实测重试即可；退避递增是因为
#: 失败往往持续数十秒（本机五次尝试中前两次都失败）。
ATTEMPTS = 5
_RETRY_DELAYS = (2.0, 5.0, 10.0, 20.0)
#: 下载超时（秒）
TIMEOUT_SECONDS = 300.0
#: 需要保留可执行位的文件后缀（Windows 侧无意义但无害）
_EXEC_SUFFIXES = (".dylib", ".so", "llama-server", "llama-server.exe")


def _ssl_context() -> ssl.SSLContext:
    """TLS 上限压到 1.2（1.3 + CDN 组合会 BAD_RECORD_MAC，与模型下载同因同解）。"""
    ctx = ssl.create_default_context()
    ctx.maximum_version = ssl.TLSVersion.TLSv1_2
    return ctx


def _download(url: str) -> bytes:
    """下载资产字节（带重试）。

    Raises:
        RuntimeError: 三次均失败。
    """
    last = ""
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with httpx.Client(
                follow_redirects=True, timeout=TIMEOUT_SECONDS, verify=_ssl_context()
            ) as client:
                return client.get(url).content
        except Exception as exc:  # noqa: BLE001 - 网络层异常种类多，统一重试
            last = f"{type(exc).__name__}: {exc}"
            print(f"    第 {attempt}/{ATTEMPTS} 次下载失败：{last}", file=sys.stderr)
            if attempt < ATTEMPTS:
                time.sleep(_RETRY_DELAYS[min(attempt - 1, len(_RETRY_DELAYS) - 1)])
    raise RuntimeError(f"下载失败（{ATTEMPTS} 次）：{last}")


def _chmod_executable(path: Path) -> None:
    """给二进制与动态库补齐可执行位（tar 里通常已带，zip 可能丢）。"""
    if path.name.endswith(_EXEC_SUFFIXES):
        path.chmod(path.stat().st_mode | 0o755)


def _extract_targz(data: bytes, dest: Path) -> None:
    """解压 tar.gz 并**保留软链接**。

    macOS 产物是 dylib 结构：真实文件是 ``libllama-common.0.4.0.dylib``，而可执行文件按
    soname ``libllama-common.0.dylib`` 查找——把软链接解成副本能跑但白占体积，丢掉软链接
    则直接 ``Library not loaded``。故这里手工解（顺带天然免疫路径穿越：只取 basename）。
    """
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar.getmembers():
            name = Path(member.name).name
            if member.isdir() or not name:
                continue
            target = dest / name
            if member.issym():
                target.unlink(missing_ok=True)
                # 软链接指向同目录的 soname 文件（归档内链接目标只在同目录）
                target.symlink_to(Path(member.linkname).name)
                continue
            if not member.isfile():
                continue
            source = tar.extractfile(member)
            if source is None:
                continue
            target.write_bytes(source.read())
            _chmod_executable(target)


def _extract_zip(data: bytes, dest: Path) -> None:
    """解压 zip（Windows 产物：llama-server.exe + 同目录 dll，无软链接）。"""
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for info in archive.infolist():
            if info.is_dir():
                continue
            name = Path(info.filename).name
            if not name:
                continue
            target = dest / name
            target.write_bytes(archive.read(info))
            _chmod_executable(target)


def main(platform_key: str, vendor_dir: str, force: bool) -> int:
    """取产物到 ``vendor_dir/<platform_key>/``。

    Returns:
        0 成功（含「已就绪，跳过」）；1 失败。
    """
    asset = ASSETS.get(platform_key)
    if asset is None:
        print(f"❌ 无 pin 的资产：{platform_key}", file=sys.stderr)
        return 1
    tag, filename, digest, kind = asset
    out_dir = Path(vendor_dir) / platform_key
    marker = out_dir / ".asset"
    exe_name = "llama-server.exe" if platform_key.startswith("win") else "llama-server"

    if not force and marker.is_file() and (out_dir / exe_name).is_file():
        if marker.read_text(encoding="utf-8").splitlines()[:3] == [tag, filename, digest]:
            print(f"✅ 已就绪（{tag}），跳过下载：{out_dir / exe_name}")
            return 0

    url = f"https://github.com/ggml-org/llama.cpp/releases/download/{tag}/{filename}"
    print(f"⏳ 下载 {filename}（tag={tag}）")
    data = _download(url)

    actual = hashlib.sha256(data).hexdigest()
    if actual != digest:
        print(f"❌ sha256 不匹配\n   期望 {digest}\n   实际 {actual}", file=sys.stderr)
        return 1
    print(f"✅ sha256 校验通过（{len(data) / 1024 / 1024:.1f} MB）")

    # 先落到临时目录再整体替换：避免半截产物留在最终路径被 PyInstaller 收进包
    staging = out_dir.with_name(f"{out_dir.name}.staging-{os.getpid()}")
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    if kind == "tar.gz":
        _extract_targz(data, staging)
    else:
        _extract_zip(data, staging)

    if not (staging / exe_name).is_file():
        shutil.rmtree(staging, ignore_errors=True)
        print(f"❌ 归档内未找到 {exe_name}，产物结构可能已变", file=sys.stderr)
        return 1

    shutil.rmtree(out_dir, ignore_errors=True)
    os.replace(staging, out_dir)
    marker.write_text(f"{tag}\n{filename}\n{digest}\n", encoding="utf-8")

    items = sorted(out_dir.iterdir())
    files = [p for p in items if p.is_file() and not p.is_symlink()]
    total_mb = sum(p.stat().st_size for p in files) / 1024 / 1024
    links = len(items) - len(files)
    print(f"✅ 就位：{out_dir}（实体 {len(files)} 个 / {total_mb:.1f} MB，软链接 {links} 个）")
    print(f"   引擎路径：{out_dir / exe_name}")
    return 0


sys.exit(main(sys.argv[1], sys.argv[2], sys.argv[3] == "1"))
PY

echo "[engine] 完成 ✓"
