.PHONY: dev dev-web dev-sidecar test build sidecar lint format gen-ipc clean install testdata testdata-clean

dev:
	npm run dev:tauri

dev-web:
	npm run dev

dev-sidecar:
	source .venv/bin/activate && cd python-sidecar && uvicorn main:app --reload --port 8765

test:
	npm run test:unit
	cargo test --manifest-path src-tauri/Cargo.toml
	cd python-sidecar && source ../.venv/bin/activate && pytest tests/

# 生产打包：先构建 Sidecar（PyInstaller onefile，自动识别本机 triple），再 tauri build。
# 注意：产物 filemind/binaries/* 已被 gitignore，构建期现场生成；CI 按各自 job 独立调用
# build-sidecar.sh（见 docs/packaging-implementation-plan.md T2/T3）。
build: sidecar
	npm run build:tauri

sidecar:
	bash scripts/build-sidecar.sh

lint:
	npm run lint
	cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
	cd python-sidecar && source ../.venv/bin/activate && ruff check . && mypy app/

format:
	npm run format
	cargo fmt --manifest-path src-tauri/Cargo.toml
	cd python-sidecar && source ../.venv/bin/activate && ruff format .

gen-ipc:
	cargo run --bin export-specta --manifest-path src-tauri/Cargo.toml -- --output src/types/ipc.ts

clean:
	rm -rf dist node_modules/.vite
	cargo clean --manifest-path src-tauri/Cargo.toml
	find python-sidecar -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

install:
	npm install
	source .venv/bin/activate || (python3.12 -m venv .venv && source .venv/bin/activate)
	pip install -r python-sidecar/requirements.txt -r python-sidecar/requirements-dev.txt

testdata:
	cargo run --example init_db --manifest-path src-tauri/Cargo.toml
	python3 scripts/gen_testdata.py

testdata-clean:
	python3 scripts/gen_testdata.py --clean
