# FileMind

Desktop file organizer with RAG Q&A — powered by Tauri 2 + React 19 + Python FastAPI Sidecar.

## Quick Start

```bash
# 1. 初始化开发环境
bash scripts/bootstrap.sh

# 2. 启动开发模式（前端 + Tauri + Sidecar）
make dev

# 3. 运行测试
make test

# 4. 代码检查
make lint
```

## Project Structure

```
filemind/
├── src/              # React 19 + TypeScript frontend
├── src-tauri/        # Rust + Tauri 2 backend
├── python-sidecar/   # Python FastAPI AI service
├── scripts/          # Build/dev/CI scripts
├── .github/          # GitHub Actions workflows
└── tests/            # E2E & integration tests
```

## Development Rules

See [AGENTS.md](./AGENTS.md) for the complete AI development rules and self-check protocol.

### Pre-commit Hooks

Husky + lint-staged automatically runs on every commit:
- ESLint + Prettier (TypeScript)
- Clippy + rustfmt (Rust)
- Ruff + mypy (Python)
- Conventional Commits message validation

### CI/CD

- **PR Check**: lint + type-check + unit-test + build-check (3 platforms)
- **Merge Build**: full build on 4 targets (macOS arm64/x64, Windows x64, Linux x64)

## Key Constraints

- No `unwrap()` / `expect()` / `panic!()` in Rust
- No `any` type in TypeScript or Python
- All IPC commands must use `#[specta::specta]` and return `Result<T, String>`
- All file paths must pass `path_guard::validate()`
- API keys stored in OS Keychain, never in Python sidecar
