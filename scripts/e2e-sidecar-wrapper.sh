#!/bin/sh
# T9.5 CI：用 HMAC 合规 stub 顶替真实 sidecar（详见 e2e/fixtures/sidecar_stub.py）。
#
# 为什么需要包装：Rust `SidecarManager::start` 直接 `Command::new($FILEMIND_SIDECAR_BINARY)`
# 无参数启动；真实二进制是 PyInstaller onefile。CI 冒烟用 stub 省去 5-10min 构建，
# 协议（stdin PSK + /health + /handshake）完全对齐。仅 CI 使用，本地仍是真实 sidecar。

exec python3 "$(dirname "$0")/../e2e/fixtures/sidecar_stub.py"
