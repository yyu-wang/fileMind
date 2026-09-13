#!/usr/bin/env bash
# CI 占位产物：为「不构建真实 sidecar 的 job」造最小 onedir 目录。
#
# 为什么需要：src-tauri/tauri.conf.json 的 bundle.resources 指向
# `../filemind/binaries/filemind-sidecar/`，tauri-build 的 build.rs 会收集并校验该
# 路径存在性——不存在即 `cargo build/clippy/test` 直接失败（exit 101）。该路径在本地
# 由 scripts/build-sidecar.sh 产出（macOS 为同名软链接），且已 gitignore，故全新检出
# 的 CI runner 上必然缺失。
#
# frontend-check / rust-check / e2e-smoke / build-check 都不需要真实 sidecar：
#   - frontend-check / rust-check / build-check 只借 cargo 生成 ipc.ts 或跑 lint，
#     产物内容不参与运行；
#   - e2e-smoke 运行时由 FILEMIND_SIDECAR_BINARY 指向 scripts/e2e-sidecar-wrapper.sh。
# 跑真实 PyInstaller 会白等 5-10min，故造占位目录即可。
#
# 用法（从仓库根）：bash scripts/stub-sidecar-product.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DST_DIR="${ROOT_DIR}/filemind/binaries/filemind-sidecar"

# 本地已有真实产物（或 dev 软链接）时不覆盖，避免踩掉真 sidecar
if [[ -e "${DST_DIR}" || -L "${DST_DIR}" ]]; then
  echo "[stub] 已存在 ${DST_DIR}，跳过"
  exit 0
fi

mkdir -p "${DST_DIR}"
# 主可执行用**自包含 shim**，不能复用 scripts/e2e-sidecar-wrapper.sh：那个 wrapper 用
# `$(dirname $0)/../e2e/fixtures/sidecar_stub.py` 定位 stub，复制到本目录后相对路径会
# 落到不存在的 `filemind/binaries/e2e/…`（曾埋的坑）。占位产物不参与运行（E2E 由
# FILEMIND_SIDECAR_BINARY 显式注入 scripts/ 下的 wrapper），万一被执行也给出明确原因。
cat > "${DST_DIR}/filemind-sidecar" <<'SHIM'
#!/bin/sh
# FileMind CI 占位产物：仅为满足 tauri-build 对 bundle.resources 的存在性校验，
# 不是可运行的 sidecar。CI 冒烟用的真 stub 见 scripts/e2e-sidecar-wrapper.sh。
echo "[stub] FileMind CI 占位产物，非可运行 sidecar" >&2
exit 1
SHIM
chmod +x "${DST_DIR}/filemind-sidecar"
echo "[stub] 占位产物 → ${DST_DIR}/"
