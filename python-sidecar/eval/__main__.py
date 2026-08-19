"""T4.5 分类准确率评估 CLI。

用法：
    python -m eval                       # 生成 500 样本集并评估（默认调 LLM 兜底）
    python -m eval --count 500 --seed 42 # 指定数量与随机种子
    python -m eval --gen-only            # 仅生成测试集，不评估
    python -m eval --no-llm              # 跳过 LLM 层（只测规则 + 启发式）
    python -m eval --dataset <path>      # 指定测试集 JSONL 路径
    python -m eval --keep                # 评估后保留临时测试集文件
"""

from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from eval.dataset import (  # noqa: E402
    CATEGORY_NAMES,
    DEFAULT_DATASET,
    EvalRecord,
    generate_dataset,
    load_jsonl,
    write_jsonl,
)
from eval.metrics import EvalReport, format_report  # noqa: E402
from eval.runner import run_eval  # noqa: E402


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    """解析命令行参数。"""
    parser = argparse.ArgumentParser(description="FileMind 分类准确率评估")
    parser.add_argument("--count", type=int, default=500, help="样本数量（默认 500）")
    parser.add_argument("--seed", type=int, default=42, help="随机种子（默认 42，可复现）")
    parser.add_argument(
        "--dataset",
        type=Path,
        default=DEFAULT_DATASET,
        help="评估集 JSONL 路径（默认 ~/.filemind/eval/classify_eval.jsonl）",
    )
    parser.add_argument("--gen-only", action="store_true", help="仅生成测试集，不评估")
    parser.add_argument("--no-llm", action="store_true", help="跳过 LLM 兜底层")
    parser.add_argument("--keep", action="store_true", help="评估后保留临时测试集文件")
    return parser.parse_args(argv)


def _summarize(records: list[EvalRecord]) -> None:
    """输出样本集的分类分布。"""
    counts: dict[str, int] = {}
    for record in records:
        counts[record.correct_category] = counts.get(record.correct_category, 0) + 1
    print(f"[eval] 样本总数 {len(records)}")
    for category in CATEGORY_NAMES:
        print(f"  {category:<4} {counts.get(category, 0)}")


def main(argv: list[str] | None = None) -> int:
    """CLI 入口。

    Args:
        argv: 命令行参数，默认取 ``sys.argv[1:]``。

    Returns:
        进程退出码（0 成功，1 失败）。
    """
    args = parse_args(argv)
    dataset = args.dataset.expanduser()
    generated = False
    if dataset.exists():
        records = load_jsonl(dataset)
        print(f"[eval] 加载评估集: {dataset}（{len(records)} 条）")
    else:
        records = generate_dataset(args.count, args.seed)
        write_jsonl(dataset, records)
        generated = True
        print(f"[eval] 生成评估集: {dataset}（count={args.count}, seed={args.seed}）")

    if args.gen_only:
        _summarize(records)
        return 0

    report: EvalReport = asyncio.run(run_eval(records, use_llm=not args.no_llm))
    print(format_report(report))

    if generated and not args.keep:
        dataset.unlink(missing_ok=True)
        print(f"[eval] 已清理临时评估集: {dataset}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
