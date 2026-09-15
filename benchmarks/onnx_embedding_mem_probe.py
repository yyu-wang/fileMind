"""ONNX int8 bge-large 进程内 embedding 的内存/耗时探针（验证脚本）。

用途：判断「Embedding 从 Ollama 换成进程内 ONNX int8」后，Sidecar 能否守住
Go/No-Go 的 RSS 门控（现行 2048MB，见 ``rules/performance.md``、
``scripts/go-no-go.py`` 第 7 项；本次实测结论即该门控由 500MB 上调的依据），
并为索引吞吐估算（``app/core/embedding_models.py`` 的 ``EST_FILES_PER_MINUTE``）提供实测值。

分阶段打印 RSS 与峰值，便于定位内存增量来自哪一层（numpy / onnxruntime / tokenizer /
会话权重 / 推理激活）。**每个 mode 必须单独进程运行**，否则导入开销会互相污染。

前置：模型文件已就位（默认 ``/tmp/bge-onnx``，取自 ``Xenova/bge-large-zh-v1.5``
的 ``onnx/model_quantized.onnx`` + tokenizer 文件）。

用法：
    python benchmarks/onnx_embedding_mem_probe.py --mode light     # 仅 tokenizers
    python benchmarks/onnx_embedding_mem_probe.py --mode hf-tok    # transformers.AutoTokenizer
    python benchmarks/onnx_embedding_mem_probe.py --threads 4      # 限制 ORT 线程数
"""

from __future__ import annotations

import argparse
import json
import time

DEFAULT_MODEL_DIR = "/tmp/bge-onnx"
QUERY = "公司本季度的营收增长情况如何？"


def rss_mb() -> float:
    """当前进程 RSS（MB）。"""
    import psutil

    return psutil.Process().memory_info().rss / 1048576


def peak_mb() -> float:
    """进程峰值 RSS（MB）；macOS 上 ``ru_maxrss`` 单位是字节。"""
    import resource

    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1048576


def mark(stages: list[dict[str, object]], name: str, extra: dict[str, object] | None = None) -> None:
    """记录并打印一个阶段的 RSS/峰值。"""
    row: dict[str, object] = {
        "stage": name,
        "rss_mb": round(rss_mb(), 1),
        "peak_mb": round(peak_mb(), 1),
    }
    if extra:
        row.update(extra)
    stages.append(row)
    print(f"[{name:<22}] rss={row['rss_mb']:>7} MB  peak={row['peak_mb']:>7} MB", flush=True)


def build_texts(count: int, chars: int = 300) -> list[str]:
    """构造接近真实分块长度的样本文本。"""
    base = "公司本季度的营收较上季度增长12%，主要来自核心产品线的销量放量，毛利率维持在38%左右。"
    return [(base * (chars // len(base) + 1))[:chars] for _ in range(count)]


def load_light_tokenizer() -> object:
    """轻量 tokenizer：只依赖 tokenizers 库（不导入 transformers）。"""
    from tokenizers import Tokenizer

    tok = Tokenizer.from_file(f"{MODEL_DIR}/tokenizer.json")
    tok.enable_truncation(max_length=512)
    tok.enable_padding(pad_id=0, pad_token="[PAD]")
    return tok


def load_hf_tokenizer() -> object:
    """transformers.AutoTokenizer（用于对比导入成本）。"""
    from transformers import AutoTokenizer

    return AutoTokenizer.from_pretrained(MODEL_DIR, local_files_only=True)


def encode_light(tok: object, texts: list[str]) -> dict[str, object]:
    """tokenizers 后端 → ORT 输入（int64）。"""
    import numpy as np

    encoded = tok.encode_batch(texts)  # type: ignore[attr-defined]
    input_ids = np.array([e.ids for e in encoded], dtype=np.int64)
    attention = np.array([e.attention_mask for e in encoded], dtype=np.int64)
    return {
        "input_ids": input_ids,
        "attention_mask": attention,
        "token_type_ids": np.zeros_like(input_ids),
    }


def encode_hf(tok: object, texts: list[str]) -> dict[str, object]:
    """transformers 后端 → ORT 输入（int64）。"""
    import numpy as np

    batch = tok(texts, padding=True, truncation=True, max_length=512, return_tensors="np")  # type: ignore[operator]
    out: dict[str, object] = {k: np.asarray(v, dtype=np.int64) for k, v in batch.items()}
    if "token_type_ids" not in out:
        out["token_type_ids"] = np.zeros_like(out["input_ids"])  # type: ignore[arg-type]
    return out


def pool_cls_and_normalize(last_hidden: object) -> object:
    """BGE 用 CLS 池化 + L2 归一化（对齐 bge 官方用法与查询侧归一化）。"""
    import numpy as np

    cls = np.asarray(last_hidden)[:, 0, :]  # type: ignore[index]
    norms = np.linalg.norm(cls, axis=1, keepdims=True)
    norms[norms == 0] = 1.0
    return cls / norms


def main() -> int:
    """按阶段加载并测量，最后打印 JSON 报告。"""
    global MODEL_DIR  # noqa: PLW0603

    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", default=DEFAULT_MODEL_DIR)
    parser.add_argument("--onnx-file", default="model_quantized.onnx")
    parser.add_argument("--mode", choices=["light", "hf-tok"], default="light")
    parser.add_argument("--batch", type=int, default=20)
    parser.add_argument("--threads", type=int, default=0, help="0 = 用 ORT 默认线程数")
    parser.add_argument("--warmup-batches", type=int, default=1)
    parser.add_argument("--timed-batches", type=int, default=3)
    parser.add_argument(
        "--no-arena",
        action="store_true",
        help="关闭 CPU 内存池与内存复用（换取更低的常驻内存）",
    )
    parser.add_argument(
        "--no-graph-opt",
        action="store_true",
        help="关闭图优化（观察是否因此产生权重副本）",
    )
    args = parser.parse_args()

    MODEL_DIR = args.model_dir
    stages: list[dict[str, object]] = []
    mark(stages, "baseline(cpython)")

    import numpy  # noqa: F401

    mark(stages, "+numpy")

    import onnxruntime as ort

    mark(stages, "+onnxruntime", {"ort": ort.__version__})

    tok = load_light_tokenizer() if args.mode == "light" else load_hf_tokenizer()
    mark(stages, f"+tokenizer({args.mode})")

    so = ort.SessionOptions()
    if args.threads:
        so.intra_op_num_threads = args.threads
        so.inter_op_num_threads = 1
    if args.no_arena:
        so.enable_cpu_mem_arena = False
        so.enable_mem_pattern = False
    if args.no_graph_opt:
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    started = time.monotonic()
    sess = ort.InferenceSession(
        f"{MODEL_DIR}/{args.onnx_file}",
        sess_options=so,
        providers=["CPUExecutionProvider"],
    )
    load_ms = int((time.monotonic() - started) * 1000)
    mark(stages, "+session", {"onnx_file": args.onnx_file, "load_ms": load_ms})

    texts = build_texts(args.batch)
    feed = encode_light(tok, texts) if args.mode == "light" else encode_hf(tok, texts)

    # 预热批：首轮含 ORT 图优化落地 / 线程池与内存池扩张，不计入稳态耗时
    for _ in range(args.warmup_batches):
        sess.run(None, feed)
    mark(stages, f"+warmup({args.warmup_batches}x)", {})

    steady_ms: list[int] = []
    for index in range(args.timed_batches):
        started = time.monotonic()
        out = sess.run(None, feed)
        steady_ms.append(int((time.monotonic() - started) * 1000))
        mark(stages, f"+infer#{index + 1}", {"infer_ms": steady_ms[-1]})

    median_ms = sorted(steady_ms)[len(steady_ms) // 2]
    started = time.monotonic()
    query_feed = encode_light(tok, [QUERY]) if args.mode == "light" else encode_hf(tok, [QUERY])
    query_out = sess.run(None, query_feed)
    query_ms = int((time.monotonic() - started) * 1000)

    vectors = pool_cls_and_normalize(out[0])
    query_vec = pool_cls_and_normalize(query_out[0])
    max_sim = float((vectors @ query_vec.T).max())
    mark(stages, "+infer(single)", {"query_ms": query_ms, "max_sim": round(max_sim, 4)})

    # 卸载检查：显式释放会话后 RSS 能否回到基线（决定「按需加载 + 空闲卸载」是否可行）
    # 注：mark 的 extra 在追加前求值，此时 stages[-1] 即 infer(single) 行
    import gc

    del sess, out, query_out, feed, query_feed, vectors, query_vec
    gc.collect()
    mark(stages, "+unload(gc)", {"reclaimed_mb": round(stages[-1]["rss_mb"] - rss_mb(), 1)})

    per_item = median_ms / max(args.batch, 1)
    report = {
        "mode": args.mode,
        "onnx_file": args.onnx_file,
        "threads": args.threads or "default",
        "arena": not args.no_arena,
        "graph_opt": not args.no_graph_opt,
        "batch": args.batch,
        "final_rss_mb": stages[-1]["rss_mb"],
        "final_peak_mb": stages[-1]["peak_mb"],
        "load_ms": load_ms,
        "steady_ms_batch_median": median_ms,
        "steady_ms_per_item": round(per_item, 1),
        "query_ms": query_ms,
        "est_chunks_per_minute": round(60000 / per_item, 1) if per_item else None,
        "stages": stages,
    }
    print("\n===REPORT===")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
