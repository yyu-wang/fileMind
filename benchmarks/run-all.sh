#!/usr/bin/env bash
# T10.5 基准一键执行：gen_testdata → scan_perf → rag_bench → 合并 JSON 报告。
#
# 产出：docs/e10-bench-report.json（meta + 6 项指标）。
#
# 规模（用户大基准请用 env 覆盖）：
#   默认 SCAN_COUNT=10000（扫描全量文件数）、RAG_COUNT=200（建索引文件数）
#   SCAN_COUNT=100000 RAG_COUNT=1000 ./benchmarks/run-all.sh   # 大基准
#
# 依赖：
#   - Rust release 工具链（scan_perf：cargo test --release）
#   - .venv（httpx/psutil，rag_bench 用）；Sidecar RAG 链路需本机
#     Ollama + 预缓存 embedding 模型，不可用时 rag_bench 自动跳过 RAG 指标。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT/.bench"
DATA_DIR="$BENCH_DIR/data"
SCAN_JSON="$BENCH_DIR/scan.json"
RAG_JSON="$BENCH_DIR/rag.json"
REPORT="$ROOT/docs/e10-bench-report.json"

SCAN_COUNT="${SCAN_COUNT:-10000}"
RAG_COUNT="${RAG_COUNT:-200}"
DEPTH="${DEPTH:-3}"
RAG_ENABLED="${RAG_ENABLED:-1}"

# 优先仓库根 .venv（rag_bench 依赖 httpx/psutil）
PY="python3"
for cand in "$ROOT/.venv/bin/python" "$ROOT/python-sidecar/.venv/bin/python"; do
  if [ -x "$cand" ]; then PY="$cand"; break; fi
done

mkdir -p "$BENCH_DIR" "$DATA_DIR" "$ROOT/docs"

echo "[bench] 1/4 生成测试数据 (count=$RAG_COUNT, depth=$DEPTH, 仅文件不入库)"
"$PY" "$ROOT/scripts/gen_testdata.py" --count "$RAG_COUNT" --depth "$DEPTH" \
  --root "$DATA_DIR" --no-db

echo "[bench] 2/4 编译并运行 scan_perf (count=$SCAN_COUNT)"
cargo test --release --test scan_perf --no-run --manifest-path "$ROOT/src-tauri/Cargo.toml" \
  --message-format=json 2>/dev/null \
  | python3 -c '
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line.startswith("{"):
        continue
    obj = json.loads(line)
    if (obj.get("reason") == "compiler-artifact"
            and obj.get("profile", {}).get("test")
            and obj.get("executable")):
        print(obj["executable"])
        break
' > "$BENCH_DIR/binpath" || true
BIN="$(cat "$BENCH_DIR/binpath" 2>/dev/null || true)"
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "[bench] 未能定位 scan_perf 测试二进制，回退 ls -t 查找"
  BIN="$(ls -t "$ROOT/src-tauri/target/release/deps"/scan_perf-* 2>/dev/null \
    | grep -v -E '\.(d|rlib|rmeta)$' | head -1 || true)"
fi
if [ -z "$BIN" ]; then
  echo "[bench] FATAL: scan_perf 二进制不存在，先检查 cargo test 是否成功" >&2
  exit 1
fi
FILEMIND_BENCH_SCAN_COUNT="$SCAN_COUNT" FILEMIND_BENCH_OUT="$SCAN_JSON" \
  "$BIN" --ignored --nocapture

if [ "$RAG_ENABLED" = "1" ]; then
  echo "[bench] 3/4 运行 rag_bench (count=$RAG_COUNT)"
  FILEMIND_BENCH_RAG_COUNT="$RAG_COUNT" \
    "$PY" "$ROOT/benchmarks/rag_bench.py" --out "$RAG_JSON" --root "$DATA_DIR"
else
  echo "[bench] 3/4 跳过 rag_bench (RAG_ENABLED=0)"
  echo '{"sidecar_rss_mb": null, "rag_ttft_ms": null, "rag_retrieve_ms": null, "rag_skipped": "RAG_ENABLED=0"}' > "$RAG_JSON"
fi

echo "[bench] 4/4 合并报告 -> $REPORT"
"$PY" - "$SCAN_JSON" "$RAG_JSON" "$REPORT" "$SCAN_COUNT" <<'PYEOF'
"""合并 scan/rag 两份 JSON 为 e10 基准报告（附 meta）。"""
import json
import platform
import subprocess
import sys

_, scan_path, rag_path, report_path, scan_count = sys.argv
with open(scan_path, encoding="utf-8") as f:
    scan = json.load(f)
with open(rag_path, encoding="utf-8") as f:
    rag = json.load(f)

n = int(scan_count)
scan_key = f"scan_full_{n // 1000}k_ms" if n % 1000 == 0 else f"scan_full_{n}_ms"
commit = "unknown"
try:
    commit = subprocess.check_output(
        ["git", "rev-parse", "--short", "HEAD"], text=True
    ).strip()
except Exception:  # noqa: BLE001 - 非 git 环境可容忍
    pass

report = {
    "meta": {
        "task": "E10 性能基准",
        "scale": "small" if n <= 10000 else "large",
        "date": platform.python_version(),
        "platform": platform.platform(),
        "commit": commit,
    },
    "scan_count": scan.get("count", n),
    scan_key: scan.get("scan_full_ms"),
    "scan_incremental_unchanged_ms": scan.get("scan_incremental_ms"),
    "sidecar_rss_mb": rag.get("sidecar_rss_mb"),
    "threshold_mb": rag.get("threshold_mb"),
    "rag_llm_model": rag.get("llm_model"),
    "rag_ttft_ms": rag.get("rag_ttft_ms"),
    "rag_retrieve_ms": rag.get("rag_retrieve_ms"),
    "rag_index_ms": rag.get("rag_index_ms"),
    "rag_skipped": rag.get("rag_skipped"),
}
with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, ensure_ascii=False, indent=2)
    f.write("\n")
print(json.dumps(report, ensure_ascii=False, indent=2))
PYEOF

echo "[bench] 完成。报告: $REPORT"
