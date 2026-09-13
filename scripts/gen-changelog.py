#!/usr/bin/env python3
"""T11 收尾：从 `git log` 按任务 ID（`T\\d+(\\.\\d+)?`）分组生成 CHANGELOG 草稿。

本仓库提交规范：conventional commits，header 内嵌任务 ID（如 `T10.2`、`T9.5`）。
脚本把提交按任务 ID 分组、按提交类型归类，输出 Keep-a-Changelog 风格的草稿，
供人工整理进 `CHANGELOG.md`（草稿 → 精修，不建议直接合入）。

用法：
    python3 scripts/gen-changelog.py                # 全量历史草稿到 stdout
    python3 scripts/gen-changelog.py --range v1.0.0..HEAD
    python3 scripts/gen-changelog.py --write docs/changelog-draft.md
"""

from __future__ import annotations  # 兼容 macOS 系统 python3（<3.10）的 `X | None` 注解

import argparse
import re
import subprocess
import sys
from collections import defaultdict

# 任务 ID：T 后跟数字（允许 .1 这种子任务）
TASK_RE = re.compile(r"T(\d+(?:\.\d+)?)")
# conventional commit：type(scope): subject
CONVENTIONAL_RE = re.compile(r"^(feat|fix|docs|refactor|perf|test|chore|build|ci)(?:\(([^)]*)\))?:\s*(.*)$")
# 提交类型展示顺序与中文名
TYPE_LABELS = [
    ("feat", "新增"),
    ("perf", "性能"),
    ("fix", "修复"),
    ("test", "测试"),
    ("docs", "文档"),
    ("refactor", "重构"),
    ("build", "构建"),
    ("ci", "CI"),
    ("chore", "杂项"),
]

# 任务 ID → 一句话主题（用于草稿小节标题；不认识的任务退化为只列提交）
TASK_TITLES = {
    "T0.8": "测试数据生成脚本",
    "T1.2": "Sidecar 打包与路径解析",
    "T1.3": "Go/No-Go 检查",
    "T1.4": "HMAC 握手协议",
    "T1.5": "Sidecar 生命周期管理",
    "T1.6": "Sidecar 进程清理",
    "T2.2": "LanceDB 向量索引",
    "T2.3": "增量索引",
    "T2.4": "FTS5 中文分词",
    "T2.5": "数据访问层 Repository",
    "T6.1": "窗口/托盘管理",
    "T6.4": "文件预览",
    "T6.5": "分类规则引擎",
    "T6.6": "SSE 流式代理",
    "T7.1": "日志脱敏",
    "T7.2": "云端模式数据脱敏",
    "T7.3": "云端 API Key 存取",
    "T7.4": "Rust 云端代理",
    "T7.5": "云端知情同意",
    "T8.5": "本地/云端 Prompt 适配",
    "T9.1": "PR 检查流水线",
    "T9.2": "合并构建流水线",
    "T9.5": "前端 E2E",
    "T10.1": "扫描性能（增量 + 并行 hash）",
    "T10.2": "RAG 首 token",
    "T10.3": "内存门控",
    "T10.4": "虚拟滚动",
    "T10.5": "基准脚本",
}


def git_log(refspec: str) -> list[tuple[str, str, str]]:
    """取 `hash / subject / body` 三元组列表。

    `%x1f` 作字段分隔（body 可能含 tab），`%x00` 作条目终止符
    （body 可能含换行，不能用 `splitlines`）。
    """
    fmt = "%H%x1f%s%x1f%b%x00"
    out = subprocess.run(
        ["git", "log", f"--format={fmt}", refspec],
        check=True, capture_output=True, text=True,
    ).stdout
    entries = []
    for entry in out.split("\x00"):
        if not entry.strip():
            continue
        # git 会在 %x00 终止符后再补一个换行，剥掉它避免混入下一条 hash
        hash_, subject, body = entry.strip("\n").split("\x1f", 2)
        entries.append((hash_[:7], subject.strip(), body.strip()))
    return entries


def parse_type(subject: str) -> tuple[str, str]:
    """返回 `(类型, 清理后的描述)`；非 conventional 提交类型为 `misc`。"""
    m = CONVENTIONAL_RE.match(subject)
    if m:
        return m.group(1), f"{m.group(2) + ': ' if m.group(2) else ''}{m.group(3)}"
    return "misc", subject


def task_id(subject: str, body: str) -> str | None:
    """返回首个任务 ID（header 优先）；无则 `None`。"""
    for text in (subject, body):
        m = TASK_RE.search(text)
        if m:
            return f"T{m.group(1)}"
    return None


def render(groups: dict[str, dict[str, list[str]]], untagged: list[tuple[str, str, str]]) -> str:
    lines = [
        "# CHANGELOG 草稿（由 scripts/gen-changelog.py 生成，人工精修后合入）",
        "",
        "> 按任务 ID 分组；每个任务下列出提交类型与描述。",
        "",
    ]
    # 任务号字典序稳定排序（T1.2 < T2.2 < T10.1）
    for task in sorted(groups, key=lambda s: tuple(int(p) for p in s.lstrip("T").split("."))):
        title = TASK_TITLES.get(task, "（未登记主题）")
        lines.append(f"## {task} — {title}")
        lines.append("")
        for label, cn in TYPE_LABELS:
            items = groups[task].get(label)
            if not items:
                continue
            lines.append(f"### {cn}")
            lines.append("")
            for item in items:
                lines.append(f"- {item}")
            lines.append("")
    if untagged:
        lines.append("## 其他（无任务 ID）")
        lines.append("")
        for hash_, subject, _body in untagged:
            lines.append(f"- `{hash_}` {subject}")
        lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--range", default="HEAD", help="git 范围（默认全量历史）")
    parser.add_argument("--write", metavar="PATH", help="写草稿到文件（默认 stdout）")
    args = parser.parse_args()

    groups: dict[str, dict[str, list[str]]] = defaultdict(lambda: defaultdict(list))
    untagged: list[tuple[str, str, str]] = []

    try:
        commits = git_log(args.range)
    except subprocess.CalledProcessError as e:
        print(f"git log 失败：{e.stderr.strip()}", file=sys.stderr)
        return 1

    for hash_, subject, body in commits:
        type_, desc = parse_type(subject)
        tid = task_id(subject, body)
        if tid is None:
            untagged.append((hash_, subject, body))
            continue
        groups[tid][type_].append(f"`{hash_}` {desc}")

    draft = render(groups, untagged)
    if args.write:
        with open(args.write, "w", encoding="utf-8") as f:
            f.write(draft)
        print(f"草稿已写入 {args.write}")
    else:
        print(draft)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
