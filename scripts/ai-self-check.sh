#!/usr/bin/env bash
set -euo pipefail

echo "=== AI 开发后自检 ==="

ERRORS=0

check() {
  local label="$1"
  local cmd="$2"
  echo "  [{label}] running..."
  if eval "$cmd" 2>&1; then
    echo "  [{label}] PASS"
  else
    echo "  [{label}] FAIL"
    ERRORS=$((ERRORS + 1))
  fi
}

echo ""
echo "--- TypeScript ---"
check "eslint" "npx eslint src/ --ext .ts,.tsx --max-warnings 0"
check "prettier" "npx prettier --check 'src/**/*.{ts,tsx,css}'"
check "tsc" "npx tsc --noEmit"
check "vitest" "npx vitest run"

echo ""
echo "--- Rust ---"
check "fmt" "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"
check "clippy" "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
check "test" "cargo test --manifest-path src-tauri/Cargo.toml"

echo ""
echo "--- Python ---"
check "ruff-lint" "ruff check python-sidecar/"
check "ruff-format" "ruff format --check python-sidecar/"
check "mypy" "mypy python-sidecar/app/"
check "pytest" "pytest python-sidecar/tests/"

echo ""
if [ $ERRORS -eq 0 ]; then
  echo "=== 全部通过 (0 errors) ==="
  exit 0
else
  echo "=== {ERRORS} 项检查失败 ==="
  exit 1
fi
