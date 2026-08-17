.PHONY: dev dev:web dev:sidecar test build lint format gen:ipc clean install

dev:
	npm run dev:tauri

dev:web:
	npm run dev

dev:sidecar:
	source .venv/bin/activate && cd python-sidecar && uvicorn main:app --reload --port 8765

test:
	npm run test:unit
	cargo test --manifest-path src-tauri/Cargo.toml
	source .venv/bin/activate && pytest python-sidecar/tests/

build:
	npm run build:tauri

lint:
	npm run lint
	cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
	source .venv/bin/activate && ruff check python-sidecar/ && mypy python-sidecar/app/

format:
	npm run format
	cargo fmt --manifest-path src-tauri/Cargo.toml
	source .venv/bin/activate && ruff format python-sidecar/

gen:ipc:
	cargo run --bin export-specta --manifest-path src-tauri/Cargo.toml -- --output src/types/ipc.ts

clean:
	rm -rf dist node_modules/.vite
	cargo clean --manifest-path src-tauri/Cargo.toml
	find python-sidecar -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

install:
	npm install
	source .venv/bin/activate || (python3 -m venv .venv && source .venv/bin/activate)
	pip install -r python-sidecar/requirements.txt -r python-sidecar/requirements-dev.txt
