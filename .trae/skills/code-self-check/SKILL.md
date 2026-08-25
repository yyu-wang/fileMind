---
name: 'code-self-check'
description: 'Runs full self-check across TypeScript, Rust, and Python — lint, type-check, tests, and security scan. Invoke after completing any code change and before committing.'
---

# Code Self-Check

Runs the project's full automated self-check pipeline across all three languages.

## When to Invoke

- After completing any code change
- Before committing code
- Before creating a PR
- User asks to "check code" / "self-check" / "verify code"

## Steps

### 1. TypeScript / React Check

```bash
# Format check
npx prettier --check "src/**/*.{ts,tsx,css}"

# ESLint (zero warnings allowed)
npx eslint src/ --ext .ts,.tsx --max-warnings 0

# Type check (strict mode)
npx tsc --noEmit

# Unit tests
npx vitest run

# Coverage (thresholds: lines 80%, functions 80%, branches 75%)
npx vitest run --coverage
```

### 2. Rust Check

```bash
# Format check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Clippy (all = "deny", zero warnings)
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Tests
cargo test --manifest-path src-tauri/Cargo.toml
```

### 3. Python Check

```bash
# Activate venv
source .venv/bin/activate

# Ruff lint
ruff check python-sidecar/

# Ruff format check
ruff format --check python-sidecar/

# Mypy strict
mypy python-sidecar/app/

# Pytest with coverage (threshold: 80%)
pytest python-sidecar/tests/ --cov=app --cov-report=term-missing
```

### 4. Security Scan

Check for common violations across all modified files:

```bash
# Check for banned patterns in Rust
grep -rn "unwrap()\|expect()\|panic!()\|dbg!()\|println!()" src-tauri/src/ --include="*.rs" | grep -v "#\[cfg(test)\]" | grep -v "mod tests"

# Check for any type in TypeScript
grep -rn ": any\|as any" src/ --include="*.ts" --include="*.tsx" | grep -v "node_modules"

# Check for Any in Python
grep -rn ": Any\|-> Any" python-sidecar/app/ --include="*.py"

# Check for console.log
grep -rn "console\.log" src/ --include="*.ts" --include="*.tsx"

# Check for print in Python
grep -rn "print(" python-sidecar/app/ --include="*.py"

# Check for type: ignore
grep -rn "type: ignore" python-sidecar/ --include="*.py"

# Check path_guard usage on file path params
grep -rn "path: String" src-tauri/src/commands/ --include="*.rs" | while read line; do
  file=$(echo "$line" | cut -d: -f1)
  if ! grep -q "path_guard::validate\|security::validate" "$file"; then
    echo "WARNING: Path parameter without validation: $line"
  fi
done
```

### 5. IPC Type Sync Check

If Rust IPC commands were modified:

```bash
# Regenerate types
make gen-ipc

# Verify no manual edits to generated file
git diff --name-only | grep "src/types/ipc.ts" && echo "WARNING: ipc.ts has uncommitted changes — ensure it was regenerated, not manually edited"
```

### 6. Report Results

Summarize results in this format:

```
=== Self-Check Report ===

TypeScript:
  [PASS] Prettier format check
  [PASS] ESLint (0 warnings)
  [PASS] Type check (strict)
  [PASS] Unit tests (X passed)
  [PASS] Coverage (lines: X%, functions: X%)

Rust:
  [PASS] Format check
  [PASS] Clippy (0 warnings)
  [PASS] Tests (X passed)

Python:
  [PASS] Ruff lint
  [PASS] Ruff format
  [PASS] Mypy strict
  [PASS] Pytest (X passed, coverage: X%)

Security:
  [PASS] No banned Rust patterns
  [PASS] No any types
  [PASS] No console.log / print
  [PASS] Path validation present

=== Result: ALL CHECKS PASSED ===
```

If any check FAILS, do NOT proceed with commit. Fix the issue first.

### 7. Quick Command

For convenience, the full check can also be run via:

```bash
bash scripts/ai-self-check.sh
```
