#!/usr/bin/env bash
set -euo pipefail

# rustup 默认安装到 ~/.cargo/bin，且无法写入 ~/.profile 时需在此补全 PATH
export PATH="$HOME/.cargo/bin:$PATH"

echo "=== Pre-commit 自检 ==="

# 1. TypeScript lint + format check
if git diff --cached --name-only | grep -qE '\.(ts|tsx)$'; then
  echo "[1/4] TypeScript lint..."
  npx eslint $(git diff --cached --name-only -- '*.ts' '*.tsx') --max-warnings 0
  npx prettier --check $(git diff --cached --name-only -- '*.ts' '*.tsx')
fi

# 2. Rust clippy + fmt check
if git diff --cached --name-only | grep -qE '^src-tauri/.*\.rs$'; then
  echo "[2/4] Rust clippy + fmt..."
  cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
  cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
fi

# 3. Python ruff + mypy
if git diff --cached --name-only | grep -qE '^python-sidecar/.*\.py$'; then
  echo "[3/4] Python ruff + mypy..."
  source .venv/bin/activate 2>/dev/null || true
  ruff check python-sidecar/
  ruff format --check python-sidecar/
  mypy python-sidecar/app/
fi

# 4. Commit message 规范检查
if [ -f .git/COMMIT_EDITMSG ]; then
  echo "[4/4] Commit message 规范检查..."
  MSG=$(cat .git/COMMIT_EDITMSG | head -1)
  PATTERN='^(feat|fix|refactor|perf|chore|docs|test|ci|build|style|revert)(\(.+\))?: .{1,72}$'
  if ! echo "$MSG" | grep -qE "$PATTERN"; then
    echo "ERROR: Commit message 不符合 Conventional Commits 规范"
    echo "格式: <type>(<scope>): <description>"
    echo "当前: $MSG"
    echo ""
    echo "可用 type: feat, fix, refactor, perf, chore, docs, test, ci, build, style, revert"
    echo "示例: feat(classify): 添加三层分类漏斗的 LLM 兜底层"
    exit 1
  fi
fi

echo "=== 自检通过 ==="
