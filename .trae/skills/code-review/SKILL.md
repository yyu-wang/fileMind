---
name: "code-review"
description: "Performs AI code review across 12 dimensions — correctness, type safety, security, complexity, architecture, error handling, tests, performance, naming, docs, dependencies, commit standards. Invoke after completing a feature and before committing."
---

# AI Code Review

Performs comprehensive code review following `rules/code-review.md` 12-dimension checklist.

## When to Invoke

- After completing a feature (before commit)
- Before creating a PR
- User asks for "code review" / "review my code" / "check this"
- After significant refactoring

## Steps

### 1. Read Review Rules

Read `rules/code-review.md` to understand the 12 review dimensions and severity levels.

### 2. Identify Changed Files

```bash
# Get all modified files (staged + unstaged)
git diff --name-only HEAD
git diff --cached --name-only
```

### 3. Run Automated Checks First

```bash
# TypeScript
npx eslint {changed_ts_files} --max-warnings 0
npx tsc --noEmit
npx prettier --check {changed_ts_files}

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Python
ruff check {changed_py_files}
ruff format --check {changed_py_files}
mypy python-sidecar/app/

# Tests
npx vitest run
cargo test --manifest-path src-tauri/Cargo.toml
pytest python-sidecar/tests/
```

### 4. Dimension-by-Dimension Review

For each changed file, review across all 12 dimensions:

#### D1: Functional Correctness
- [ ] Code implements the requirement from the task spec
- [ ] Edge cases handled: null/undefined, empty arrays, zero values, max values
- [ ] Error paths have test coverage
- [ ] Concurrent scenarios considered (Sidecar restart, file race conditions)

#### D2: Type Safety
- [ ] No `any` (TS), no `Any` (Python), no `unwrap()`/`expect()` (Rust)
- [ ] No `!` non-null assertion (TS), no `# type: ignore` (Python)
- [ ] All public functions have complete type annotations
- [ ] All types derive `Serialize + Deserialize + specta::Type` (Rust IPC)

#### D3: Security Compliance (read `rules/security.md`)
- [ ] All path params pass through `path_guard::validate()`
- [ ] No hardcoded API keys or secrets
- [ ] Mode switch calls `validate_mode_switch()`
- [ ] Cloud data is sanitized (file names anonymized, paths masked)
- [ ] Logs don't contain sensitive data

#### D4: Complexity (read `rules/complexity.md`)
- [ ] File line count within limits (TSX < 300, Rust < 500, Python < 500)
- [ ] Function line count < 60
- [ ] Cyclomatic complexity < 15
- [ ] Parameters < 6
- [ ] Nesting depth < 4
- [ ] React: useState < 7, useEffect < 5, props < 8

#### D5: Architecture Consistency
- [ ] Frontend calls backend via IPC, not direct file access
- [ ] Python Sidecar doesn't store API keys
- [ ] DB changes via migration scripts, not direct ALTER
- [ ] IPC commands have `#[specta::specta]` and return `Result<T, String>`
- [ ] `src/types/ipc.ts` regenerated if IPC changed

#### D6: Error Handling (read `rules/error-handling.md`)
- [ ] Rust: `Result<T, AppError>` chain, no swallowed errors
- [ ] Python: HTTPException with correct status + user-friendly message
- [ ] Frontend: IPC errors have toast/inline display, no silent failures
- [ ] Error codes follow the error code registry format (`{MODULE}-{TYPE}-{NUM}`)

#### D7: Test Coverage
- [ ] New functions have unit tests
- [ ] Edge cases tested (empty list, invalid path, permission denied)
- [ ] Coverage >= 80%
- [ ] Tests mock external services (Ollama, cloud API)

#### D8: Performance (read `rules/performance.md`)
- [ ] Large lists use virtual scrolling
- [ ] File scan is incremental, not full rescan
- [ ] Embedding batch processing, not one-by-one
- [ ] Rust hot path: no unnecessary allocations
- [ ] Python: async IO, not blocking

#### D9: Naming Conventions (read `rules/{language}.md`)
- [ ] Components: PascalCase, Hooks: useXxx, Stores: xxxStore
- [ ] Variables have business meaning (not `data`, `temp`, `x`)
- [ ] Booleans use is/has/can/should prefix

#### D10: Documentation Sync
- [ ] Public functions have docstring/JSDoc
- [ ] IPC changes reflected in API spec doc
- [ ] DB table changes reflected in data model doc
- [ ] New Feature Flags added to flags.ts

#### D11: Dependency Management (read `rules/dependency.md`)
- [ ] New dependencies evaluated (necessity, maintenance, security, license)
- [ ] No `latest` tags, versions locked
- [ ] No known vulnerabilities

#### D12: Commit Standards (read `rules/release.md`)
- [ ] Commit message: Conventional Commits format
- [ ] One PR = one feature/fix
- [ ] PR description includes change summary + test plan
- [ ] CI all green

### 5. Generate Review Report

```markdown
## AI Code Review Report

### Review Scope
- Files: {list of changed files}
- Feature: {task ID and description}

### Automated Checks
| Check | Result |
|-------|--------|
| ESLint | PASS/FAIL |
| Clippy | PASS/FAIL |
| Ruff + Mypy | PASS/FAIL |
| TypeScript | PASS/FAIL |
| Tests | PASS/FAIL (X passed) |

### Dimension Review

| # | Dimension | Result | Issues |
|---|-----------|--------|--------|
| 1 | Functional Correctness | PASS/FAIL | ... |
| 2 | Type Safety | PASS/FAIL | ... |
| 3 | Security Compliance | PASS/FAIL | ... |
| 4 | Complexity | PASS/FAIL | ... |
| 5 | Architecture Consistency | PASS/FAIL | ... |
| 6 | Error Handling | PASS/FAIL | ... |
| 7 | Test Coverage | PASS/FAIL | ... |
| 8 | Performance | PASS/FAIL | ... |
| 9 | Naming Conventions | PASS/FAIL | ... |
| 10 | Documentation Sync | PASS/FAIL | ... |
| 11 | Dependency Management | PASS/FAIL | ... |
| 12 | Commit Standards | PASS/FAIL | ... |

### Issues Found

#### Blocker (must fix before commit)
1. [file:line] {description}

#### Critical (must fix)
1. [file:line] {description}

#### Major (should fix)
1. [file:line] {description}

#### Minor (suggestion)
1. [file:line] {description}

### Conclusion
- [ ] APPROVED — Ready to commit
- [ ] CHANGES REQUESTED — Fix N issues and re-review
```

### 6. Fix Issues

If any Blocker or Critical issues found:
1. Fix each issue
2. Re-run automated checks
3. Re-review fixed code
4. Only proceed when all Blocker/Critical issues resolved

### 7. Final Sign-off

```
=== Code Review Sign-off ===
Reviewer: AI (automated)
Date: {timestamp}
Files reviewed: {count}
Issues found: {blocker} Blocker / {critical} Critical / {major} Major / {minor} Minor
Status: APPROVED / CHANGES REQUESTED
```
