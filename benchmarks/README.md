# E10 性能基准（T10.5）

FileMind 的性能验收基准：一条命令产出 `docs/e10-bench-report.json`（6 项指标），
覆盖 E10 三条主线——扫描性能（T10.1）、RAG 首 token（T10.2）、内存（T10.3）。

## 6 项指标

| 指标                            | 来源                                  | 说明                                    | 验收线       |
| ------------------------------- | ------------------------------------- | --------------------------------------- | ------------ |
| `scan_full_10k_ms`              | `scan_perf.rs`                        | 10k 文件全量扫描（哈希 + 写库）耗时     | —            |
| `scan_full_100k_ms`             | `scan_perf.rs`（`SCAN_COUNT=100000`） | 100k 文件全量扫描耗时                   | <30s         |
| `scan_incremental_unchanged_ms` | `scan_perf.rs`                        | 二次扫描（内容未变，跳过哈希/写库）耗时 | 显著低于全量 |
| `rag_ttft_ms`                   | `rag_bench.py`                        | 问答请求发出到首个 token 的耗时         | <3s          |
| `rag_retrieve_ms`               | `rag_bench.py`                        | 检索阶段耗时（改写 → 向量/FTS → 重排）  | —            |
| `sidecar_rss_mb`                | `rag_bench.py`                        | Sidecar 冷启动 RSS（`/metrics`）        | <500MB       |

`rag_ttft_ms`/`rag_retrieve_ms`/`sidecar_rss_mb` 需要本机 Ollama + 预缓存
embedding 模型；不可用时 `rag_bench.py` 将 RAG 指标置 `null` 并注明
`rag_skipped` 原因（仅 RSS 仍照常产出）。

## M1 Pro 实测结论（2026-09-01，qwen2.5:7b 门控基准）

5/6 项达标（scan 10k 846.9ms、增量 144.2ms、RSS 161MB、索引与检索无硬线）；
**`rag_ttft_ms` 41.7s 不达标**（目标 <3s、告警线 4s），根因为本机
prefill 物理限制，非链路实现问题：

- **主导项 prefill ~32s**：实测 Ollama 0.32 + qwen2.5:7b（Metal 后端）prefill
  仅 ~200 tok/s，RAG 上下文（5 chunk × 500 字 + 模板）≈ 6.4k tokens →
  6446 tok ÷ 196 tok/s ≈ 32s。已排除的变量：
  - `num_ctx` 8192 vs 32768：实测 prefill 196 vs 197 tok/s，无差异；
  - `OLLAMA_FLASH_ATTENTION=1`：实测无提升（196 tok/s 不变）；
  - 内存压力：32GB 机器 59% free，无 swap 抖动。
- **次项 rerank 冷加载 ~5.8s**：基准每次重启 sidecar，bge-reranker-v2-m3
  首次检索时加载（`rerank_service` 懒加载设计，10min 空闲卸载），
  计入 `rag_retrieve_ms`（7.8s）中；用户连续问答场景下该成本仅首次发生。
- **达标路径**（后续 T10.2 优化方向）：削减 LLM 上下文（`rerank_top_k`
  5→3 + chunk 截断，prefill 与 token 数线性相关）；KV cache 量化
  （`OLLAMA_KV_CACHE_TYPE=q8_0`）；云端模式（OpenAI，目标 <2s）；
  或接受本机 TTFT 实测值并将 3s 目标修订为「云端模式 / 更高带宽硬件」。

## 小规模（AI 本地冒烟，约 1 分钟）

```bash
./benchmarks/run-all.sh
# SCAN_COUNT=2000 可再加快；RAG_ENABLED=0 跳过 RAG 仅测扫描+RSS
```

## 大规模（用户执行，出验收报告）

```bash
# 100k 文件全量扫描 + 1000 文件建索引 + 真实 Ollama 问答
SCAN_COUNT=100000 RAG_COUNT=1000 ./benchmarks/run-all.sh

# 门控基准（09_测试体系 §7 规定本地模式用 qwen2.5:7b，默认 qwen3.8-27b
# 为本机日常模型，27B 在 M1 Pro 上 TTFT 远超 4s 门槛）：
FILEMIND_BENCH_LLM_MODEL=qwen2.5:7b ./benchmarks/run-all.sh
# 报告 rag_llm_model 字段记录本次使用的 LLM 模型
```

- 100k 扫描预算 <30s；全量 RAG 依赖本机 Ollama（`qwen3.8-27b`）与
  `bge-large-zh-v1.5`（见 python-sidecar README 预缓存说明）。
- 报告含 `meta.scale`（`small`/`large`）区分两次执行；大基准会覆盖
  `docs/e10-bench-report.json`（如需留档请先备份）。

## 单步执行（调试用）

```bash
# 1) 扫描基准（Rust 集成测试，--ignored 门控）
cargo test --release --test scan_perf --no-run --manifest-path src-tauri/Cargo.toml
# release 模式 cargo test 会连带链接 filemind bin（Tauri 测试 harness 已知问题），
# 故用 --no-run 只编 lib+test，再直接跑测试二进制：
FILEMIND_BENCH_SCAN_COUNT=2000 FILEMIND_BENCH_OUT=/tmp/scan.json \
  src-tauri/target/release/deps/scan_perf-<hash> --ignored --nocapture

# 2) 数据生成（仅文件不入库，隔离）
.venv/bin/python scripts/gen_testdata.py --count 200 --depth 3 --root .bench/data --no-db

# 3) RAG 基准（RSS + 首 token + 检索）
.venv/bin/python benchmarks/rag_bench.py --out .bench/rag.json --root .bench/data
```

## 原理

- `src-tauri/tests/scan_perf.rs`：临时目录生成 `count` 个文件 + 临时 SQLite，
  直接调用 `scan_and_persist`（M1 暴露的扫描入口），全量/增量各测一次，JSON
  写 `FILEMIND_BENCH_OUT`。**数据自包含**，不依赖 `gen_testdata.py`。
- `benchmarks/rag_bench.py`：复用 `scripts/_sidecar_launcher.py`（与
  `go-no-go.py` 共享的 Sidecar 启动 + HMAC 签名），先 `/metrics` 记 RSS，
  再 `/index/build` + `/chat/stream` 测检索与首 token。
- `benchmarks/run-all.sh`：gen_testdata（RAG 数据）→ scan_perf（扫描数据）→
  rag_bench → 合并两份 JSON + meta 到 `docs/e10-bench-report.json`。
