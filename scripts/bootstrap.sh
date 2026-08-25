#!/usr/bin/env bash
set -euo pipefail

echo "=== FileMind 开发环境初始化 ==="

# 1. 检查前置依赖
echo "[1/6] 检查前置依赖..."
command -v node >/dev/null 2>&1 || { echo "ERROR: Node.js not found. Install: https://nodejs.org"; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "ERROR: Rust not found. Install: https://rustup.rs"; exit 1; }
# ENG-3：pyproject 锁定 py312，代码用 typing.Self（需 ≥3.11），系统默认 python3 可能过旧
command -v python3.12 >/dev/null 2>&1 || { echo "ERROR: Python 3.12 not found (pyproject requires py312)."; exit 1; }

NODE_VERSION=$(node -v | sed 's/v//' | cut -d. -f1)
if [ "$NODE_VERSION" -lt 20 ]; then
  echo "ERROR: Node.js >= 20 required, got $NODE_VERSION"
  exit 1
fi

echo "  Node.js: $(node -v)"
echo "  Rust: $(rustc --version)"
echo "  Python: $(python3.12 --version)"

# 2. 安装前端依赖
echo "[2/6] 安装前端依赖..."
npm install

# 3. 安装 Python Sidecar 依赖
echo "[3/6] 配置 Python 环境..."
python3.12 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip
pip install -r python-sidecar/requirements.txt
pip install -r python-sidecar/requirements-dev.txt

# 4. 安装 Tauri CLI
echo "[4/6] 检查 Tauri CLI..."
cargo install tauri-cli@^2.0.0 --locked 2>/dev/null || echo "  Tauri CLI already installed"

# 5. 检查 Ollama
echo "[5/6] 检查 Ollama..."
if ! command -v ollama >/dev/null 2>&1; then
  echo "  WARNING: Ollama not found. Local inference will not work."
  echo "  Install: https://ollama.com/download"
else
  echo "  Ollama: $(ollama --version 2>/dev/null || echo 'installed')"
fi

# 6. 生成 IPC 类型
echo "[6/6] 生成 IPC 类型..."
cargo build --manifest-path src-tauri/Cargo.toml 2>/dev/null || echo "  (Rust 首次编译需要一些时间，IPC 类型生成将在编译后可用)"

echo ""
echo "=== 初始化完成! ==="
echo "运行 'make dev' 启动开发环境"
