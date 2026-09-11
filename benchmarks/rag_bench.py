#!/usr/bin/env python3
"""T10.5 RAG 基准（产出 3 项指标）：sidecar_rss_mb / rag_retrieve_ms / rag_ttft_ms。

流程：
1. 拉起 Sidecar（复用 ``scripts/_sidecar_launcher.py``）；
2. ``GET /metrics`` 记冷启动 RSS（验收线 <500MB）；
3. 可选 RAG：从 ``--root`` 收集文件 → ``POST /index/build`` 建索引 →
   ``POST /chat/stream`` 流式问答，记检索耗时（done.retrieve_ms）与首 token
   耗时（客户端实测）。Embedding 模型 / Ollama 不可用时三项 RAG 指标置
   ``null`` 并注明原因，仅 RSS 部分照常产出（脚本仍成功退出）。

用法：
    python3 benchmarks/rag_bench.py --out .bench/rag.json
    python3 benchmarks/rag_bench.py --skip-rag            # 仅 RSS
    FILEMIND_BENCH_RAG_COUNT=100 python3 benchmarks/rag_bench.py
    FILEMIND_BENCH_LLM_MODEL=qwen2.5:7b python3 benchmarks/rag_bench.py

输出：
    单文件 JSON（见 ``--out``），由 ``benchmarks/run-all.sh`` 合并进
    ``docs/e10-bench-report.json``。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT_DIR / "scripts"))

from _sidecar_launcher import (
    SIDECAR_BASE,
    DevSidecar,
    SignedClient,
    _wait_health,
)

EMBEDDING_MODEL = "bge-large-zh-v1.5"
#: LLM 模型可被 FILEMIND_BENCH_LLM_MODEL 覆盖（09_测试体系 §7 门控基准用
#: qwen2.5:7b；默认取本机日常模型，两者结果均如实写入报告 llm_model 字段）
LLM_MODEL = os.environ.get("FILEMIND_BENCH_LLM_MODEL", "qwen3.8-27b")
DEFAULT_TABLE = "documents_bge-large-zh-v1.5_bench"
# 与分类语料语义对齐的基准问句（命中「文档」类文件名/内容）
BENCH_QUERY = "项目计划与年度总结相关文档"


def _collect_files(root: Path, count: int) -> list[dict[str, str]]:
    """从 ``root`` 收集前 ``count`` 个文件（file_id = path 的 sha1）。

    Args:
        root: 基准数据目录（gen_testdata.py 生成）。
        count: 最多收集的文件数。

    Returns:
        ``[{"file_id", "path"}]`` 列表，用于 POST /index/build。
    """
    files: list[dict[str, str]] = []
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in filenames:
            path = Path(dirpath) / name
            files.append(
                {
                    "file_id": hashlib.sha1(str(path).encode("utf-8")).hexdigest(),
                    "path": str(path),
                }
            )
            if len(files) >= count:
                return files
    return files


def _run_rag(
    client: SignedClient, root: Path, count: int, table: str
) -> dict[str, object]:
    """建索引 + 流式问答，返回 RAG 三项指标（不可用时全部置 null）。

    Args:
        client: 签名请求客户端（与当前 Sidecar 实例配对）。
        root: 基准数据目录。
        count: 建索引的文件数。
        table: 目标向量表名（基准隔离，不碰用户真实索引）。

    Returns:
        ``{"rag_ttft_ms", "rag_retrieve_ms", "rag_index_ms", "rag_skipped"}``。
    """
    files = _collect_files(root, count)
    if not files:
        return {
            "rag_ttft_ms": None,
            "rag_retrieve_ms": None,
            "rag_index_ms": None,
            "rag_skipped": f"目录 {root} 无文件，跳过索引与问答",
        }

    # --- 建索引（Embedding 不可用时 503 → 跳过 RAG）---
    index_body = {
        "files": files,
        "embedding_model": EMBEDDING_MODEL,
        "table_name": table,
    }
    t0 = time.monotonic()
    try:
        index_resp = client.request("POST", "/index/build", index_body, timeout=600.0)
    except Exception as exc:  # noqa: BLE001 - 网络/超时统一按不可用跳过
        return {
            "rag_ttft_ms": None,
            "rag_retrieve_ms": None,
            "rag_index_ms": None,
            "rag_skipped": f"/index/build 请求失败: {exc}",
        }
    index_ms = int((time.monotonic() - t0) * 1000)
    if index_resp.status_code != 200:
        return {
            "rag_ttft_ms": None,
            "rag_retrieve_ms": None,
            "rag_index_ms": index_ms,
            "rag_skipped": f"/index/build {index_resp.status_code}: {index_resp.text[:120]}",
        }

    # --- 流式问答 ---
    chat_body = {
        "query": BENCH_QUERY,
        "history": [],
        "table_name": table,
        "embedding_model": EMBEDDING_MODEL,
        "inference_mode": "local",
        "llm_model": LLM_MODEL,
        "top_k": 20,
        "rerank_top_k": 5,
        "max_retries": 2,
        "fts_chunks": [],
        "session_id": None,
    }
    t0 = time.monotonic()
    first_token_ms: int | None = None
    retrieve_ms: int | None = None
    skipped: str | None = None
    try:
        resp = client.request("POST", "/chat/stream", chat_body, timeout=600.0)
        if resp.status_code != 200:
            skipped = f"/chat/stream {resp.status_code}: {resp.text[:120]}"
        else:
            for line in resp.iter_lines():
                if not line or line.startswith(("event:", ":")):
                    continue
                if line.startswith("data: "):
                    try:
                        data = json.loads(line[6:])
                    except json.JSONDecodeError:
                        continue
                    if "content" in data and first_token_ms is None:
                        first_token_ms = int((time.monotonic() - t0) * 1000)
                    if "retrieve_ms" in data and retrieve_ms is None:
                        retrieve_ms = data["retrieve_ms"]
                    if "code" in data and data.get("code") in (
                        "OLLAMA_UNAVAILABLE",
                        "INTERNAL_ERROR",
                    ):
                        skipped = f"/chat/stream error: {data.get('message')}"
                        break
    except Exception as exc:  # noqa: BLE001 - 网络中断按不可用跳过
        skipped = f"/chat/stream 请求失败: {exc}"

    return {
        "rag_ttft_ms": first_token_ms,
        "rag_retrieve_ms": retrieve_ms,
        "rag_index_ms": index_ms,
        "rag_skipped": skipped,
    }


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    """解析命令行参数。"""
    parser = argparse.ArgumentParser(
        description="T10.5 RAG 基准（RSS + 首 token + 检索耗时）"
    )
    parser.add_argument(
        "--out",
        type=str,
        default=str(ROOT_DIR / ".bench" / "rag.json"),
        help="输出 JSON 路径（默认 .bench/rag.json）",
    )
    parser.add_argument(
        "--root",
        type=str,
        default=str(ROOT_DIR / ".bench" / "data"),
        help="基准数据目录（gen_testdata.py 生成，默认 .bench/data）",
    )
    parser.add_argument(
        "--table",
        type=str,
        default=DEFAULT_TABLE,
        help="LanceDB 表名（默认 documents_bge-large-zh-v1.5_bench，隔离用）",
    )
    parser.add_argument(
        "--count",
        type=int,
        default=0,
        help="建索引文件数上限（默认 env FILEMIND_BENCH_RAG_COUNT 或 200）",
    )
    parser.add_argument(
        "--skip-rag", action="store_true", help="仅测 RSS，跳过索引与问答"
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """基准入口：起 Sidecar → metrics → 可选 RAG → 写 JSON。"""
    args = parse_args(argv)
    count = args.count or int(os.environ.get("FILEMIND_BENCH_RAG_COUNT", "200"))

    sc = DevSidecar()
    result: dict[str, object] = {"sidecar_rss_mb": None, "llm_model": LLM_MODEL}
    try:
        sc.start()
        if _wait_health() is None:
            result["rag_skipped"] = "Sidecar /health 无响应"
            result.update(_null_rag())
            return _write_report(args.out, result)

        client = SignedClient()
        metrics_resp = client.request("GET", "/metrics", timeout=10.0)
        if metrics_resp.status_code == 200:
            data = metrics_resp.json()
            result["sidecar_rss_mb"] = data.get("rss_mb")
            result["threshold_mb"] = data.get("threshold_mb")
        else:
            result["rag_skipped"] = f"/metrics {metrics_resp.status_code}"
            result.update(_null_rag())
            return _write_report(args.out, result)

        if not args.skip_rag:
            result.update(_run_rag(client, Path(args.root), count, args.table))
        else:
            result.update(_null_rag())
    finally:
        sc.stop()
    return _write_report(args.out, result)


def _null_rag() -> dict[str, object]:
    """RAG 三项指标的 null 占位（合并进报告时字段结构保持一致）。"""
    return {"rag_ttft_ms": None, "rag_retrieve_ms": None, "rag_index_ms": None}


def _write_report(out: str, result: dict[str, object]) -> int:
    """写 JSON 报告文件。"""
    out_path = Path(out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"[rag_bench] {SIDECAR_BASE} 报告 -> {out_path}")
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
