#!/usr/bin/env python3
"""FileMind 测试数据生成脚本（T0.8）。

在 ``~/Desktop/filemind-test-data/`` 下按内置分类生成假文件，并将元数据写入
``~/.filemind/filemind.db`` 的 ``files``/``categories`` 表，用于文件列表、
分类与全文检索链路的本地联调。

特性：
- 固定随机种子（默认 42），重复执行生成相同文件集合，按 path 幂等 upsert；
- ``--clean`` 删除测试目录并清理 ``files`` 表中对应记录（FTS 由触发器联动）；
- ``--dry-run`` 仅输出计划，不写磁盘与数据库。

前置条件：
- 数据库已初始化。若 ``~/.filemind/filemind.db`` 不存在，先执行
  ``cargo run --example init_db --manifest-path src-tauri/Cargo.toml``，
  或启动一次应用（``make dev``）。

用法：
    python3 scripts/gen_testdata.py                # 生成默认 200 个文件
    python3 scripts/gen_testdata.py --count 500    # 指定数量
    python3 scripts/gen_testdata.py --dry-run      # 预览计划
    python3 scripts/gen_testdata.py --clean        # 清理测试数据
"""

from __future__ import annotations

import argparse
import hashlib
import logging
import os
import random
import shutil
import sqlite3
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path

logger = logging.getLogger("gen_testdata")

TESTDATA_ROOT = Path.home() / "Desktop" / "filemind-test-data"
DB_PATH = Path.home() / ".filemind" / "filemind.db"

# 单文件字节上限：控制磁盘占用（测试只关心元数据真实性，不追求真实体积）
MAX_FILE_BYTES = 256 * 1024

# 与 file_repo::INSERT_FILE_SQL 语义一致，另将 category 一并刷新：
# 脚本目标是"重复执行收敛到当前规格"，与扫描保留旧分类的语义不同。
INSERT_FILE_SQL = """
    INSERT INTO files (id, path, file_name, file_size, content_hash, category, is_deleted, created_at, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, datetime('now'), datetime('now'))
    ON CONFLICT(path) DO UPDATE SET
        file_name = excluded.file_name,
        file_size = excluded.file_size,
        content_hash = excluded.content_hash,
        category = excluded.category,
        is_deleted = 0,
        updated_at = datetime('now')
"""

UPSERT_CATEGORY_SQL = """
    INSERT INTO categories (id, name, parent_id, icon, color, sort_order, is_builtin, created_at, updated_at)
    VALUES (?1, ?2, NULL, ?3, ?4, ?5, 1, datetime('now'), datetime('now'))
    ON CONFLICT(name) DO UPDATE SET
        icon = excluded.icon,
        color = excluded.color,
        sort_order = excluded.sort_order,
        updated_at = datetime('now')
"""

NAME_SUFFIXES = (
    "",
    "-final",
    "_v2",
    "_v3",
    "-副本",
    "（新）",
    "-2024",
    "-draft",
    "-备份",
)


@dataclass(frozen=True)
class CategorySpec:
    """内置分类规格：目录名、展示属性、文件名池与扩展名池。"""

    name: str
    icon: str
    color: str
    stems: tuple[str, ...]
    extensions: tuple[str, ...]
    size_range: tuple[int, int]


BUILTIN_CATEGORIES: tuple[CategorySpec, ...] = (
    CategorySpec(
        name="文档",
        icon="📄",
        color="#3B82F6",
        stems=(
            "年度总结",
            "项目计划",
            "会议纪要",
            "合同草案",
            "产品需求文档",
            "财报",
            "invoice",
            "简历",
            "读书笔记",
            "周报",
        ),
        extensions=(".pdf", ".docx", ".md", ".txt", ".xlsx", ".pptx"),
        size_range=(2 * 1024, 128 * 1024),
    ),
    CategorySpec(
        name="图片",
        icon="🖼️",
        color="#10B981",
        stems=(
            "旅行照片",
            "屏幕截图",
            "头像",
            "壁纸",
            "产品图",
            "family-photo",
            "banner",
            "证件照",
            "表情包",
            "杂拍",
        ),
        extensions=(".jpg", ".png", ".gif", ".heic", ".webp"),
        size_range=(16 * 1024, 256 * 1024),
    ),
    CategorySpec(
        name="视频",
        icon="🎬",
        color="#F59E0B",
        stems=(
            "屏幕录制",
            "生日聚会",
            "产品演示",
            "旅行vlog",
            "课程录像",
            "demo",
            "会议录屏",
            "宣传片",
        ),
        extensions=(".mp4", ".mov", ".avi", ".mkv"),
        size_range=(64 * 1024, 256 * 1024),
    ),
    CategorySpec(
        name="音频",
        icon="🎵",
        color="#8B5CF6",
        stems=(
            "会议录音",
            "播客",
            "练习曲",
            "录音备忘",
            "podcast",
            "白噪音",
            "试听片段",
        ),
        extensions=(".mp3", ".wav", ".m4a", ".flac"),
        size_range=(32 * 1024, 256 * 1024),
    ),
    CategorySpec(
        name="代码",
        icon="💻",
        color="#06B6D4",
        stems=(
            "main",
            "utils",
            "parser",
            "算法练习",
            "爬虫脚本",
            "test_case",
            "实验代码",
            "leetcode",
        ),
        extensions=(".rs", ".py", ".ts", ".js", ".html", ".css", ".sh"),
        size_range=(256, 32 * 1024),
    ),
    CategorySpec(
        name="压缩包",
        icon="🗂️",
        color="#6B7280",
        stems=(
            "项目备份",
            "照片归档",
            "安装包",
            "资源合集",
            "backup",
            "素材包",
            "旧电脑迁移",
        ),
        extensions=(".zip", ".tar.gz", ".rar", ".7z"),
        size_range=(32 * 1024, 256 * 1024),
    ),
    CategorySpec(
        name="设计",
        icon="🎨",
        color="#EC4899",
        stems=(
            "首页设计稿",
            "Logo草案",
            "图标集",
            "原型图",
            "mockup",
            "海报设计",
            "品牌规范",
        ),
        extensions=(".psd", ".ai", ".sketch", ".fig", ".xd"),
        size_range=(64 * 1024, 256 * 1024),
    ),
    CategorySpec(
        name="其他",
        icon="📦",
        color="#9CA3AF",
        stems=("临时文件", "unknown", "待整理", "下载备份", "notes", "随手记", "misc"),
        extensions=(".dat", ".log", ".csv", ".xml", ".bin"),
        size_range=(128, 16 * 1024),
    ),
)


@dataclass(frozen=True)
class PlannedFile:
    """待写入的文件计划：分类、文件名与字节数。"""

    spec: CategorySpec
    file_name: str
    size: int


@dataclass(frozen=True)
class FileRow:
    """落盘后的文件记录：与 ``files`` 表列一一对应。"""

    id: str
    path: str
    file_name: str
    file_size: int
    content_hash: str
    category: str


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    """解析命令行参数。

    Args:
        argv: 命令行参数列表，默认取 ``sys.argv[1:]``。

    Returns:
        解析后的命名空间（count/clean/dry_run/seed）。
    """
    parser = argparse.ArgumentParser(description="FileMind 测试数据生成脚本")
    parser.add_argument(
        "--count", type=int, default=200, help="生成文件数量（默认 200）"
    )
    parser.add_argument(
        "--clean", action="store_true", help="删除测试目录并清理 DB 记录"
    )
    parser.add_argument("--dry-run", action="store_true", help="仅输出计划，不写入")
    parser.add_argument(
        "--seed", type=int, default=42, help="随机种子（默认 42，保证可复现）"
    )
    parser.add_argument(
        "--root",
        type=str,
        default=None,
        help="覆盖测试数据目录（默认 ~/Desktop/filemind-test-data）",
    )
    parser.add_argument(
        "--db",
        type=str,
        default=None,
        help="覆盖数据库路径（默认 ~/.filemind/filemind.db）",
    )
    parser.add_argument(
        "--depth",
        type=int,
        default=0,
        help="文件嵌套目录深度（默认 0 = 平铺；T10.5 基准模拟真实目录树）",
    )
    parser.add_argument(
        "--no-db",
        action="store_true",
        help="仅写文件不入库（T10.5 基准隔离用，不触碰用户数据库）",
    )
    return parser.parse_args(argv)


def connect_db() -> sqlite3.Connection:
    """打开数据库并校验核心表存在。

    Returns:
        已开启外键约束的连接。

    Raises:
        FileNotFoundError: 数据库文件或核心表不存在（需先初始化）。
    """
    if not DB_PATH.exists():
        raise FileNotFoundError(
            f"数据库不存在: {DB_PATH}\n"
            "请先初始化：cargo run --example init_db --manifest-path src-tauri/Cargo.toml\n"
            "或启动一次应用（make dev）"
        )
    conn = sqlite3.connect(DB_PATH, timeout=5.0)
    conn.execute("PRAGMA foreign_keys=ON")
    tables = {
        row[0]
        for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")
    }
    missing = {"files", "categories", "file_fts"} - tables
    if missing:
        conn.close()
        raise FileNotFoundError(f"数据库缺少核心表 {sorted(missing)}，请先初始化数据库")
    return conn


def build_unique_filename(
    rng: random.Random, spec: CategorySpec, used: set[str]
) -> str:
    """为分类生成一个目录内唯一的文件名。

    Args:
        rng: 随机数生成器。
        spec: 目标分类规格。
        used: 该分类目录下已占用的文件名集合。

    Returns:
        形如 ``词组后缀.ext`` 的唯一文件名。
    """
    while True:
        stem = rng.choice(spec.stems)
        suffix = rng.choice(NAME_SUFFIXES)
        ext = rng.choice(spec.extensions)
        name = f"{stem}{suffix}{ext}"
        if name not in used:
            used.add(name)
            return name


def plan_files(rng: random.Random, count: int) -> list[PlannedFile]:
    """按分类轮转生成文件计划（不触碰磁盘）。

    Args:
        rng: 随机数生成器。
        count: 文件总数。

    Returns:
        文件计划列表，分类近似均匀分布。
    """
    used_by_category: dict[str, set[str]] = {c.name: set() for c in BUILTIN_CATEGORIES}
    plans: list[PlannedFile] = []
    for i in range(count):
        spec = BUILTIN_CATEGORIES[i % len(BUILTIN_CATEGORIES)]
        name = build_unique_filename(rng, spec, used_by_category[spec.name])
        size = rng.randrange(
            spec.size_range[0], min(spec.size_range[1], MAX_FILE_BYTES) + 1
        )
        plans.append(PlannedFile(spec=spec, file_name=name, size=size))
    return plans


def _nested_dir(index: int, depth: int) -> str:
    """按 index 确定性映射到深度为 ``depth`` 的子目录链（每层 10 个分支）。

    默认 depth=0 返回空串（平铺，保持既有目录结构兼容）。
    """
    return "/".join(f"d{(index // (level + 1)) % 10}" for level in range(depth))


def materialize_files(plans: list[PlannedFile], depth: int = 0) -> list[FileRow]:
    """将文件计划落盘并计算哈希，生成 DB 记录行。

    Args:
        plans: 文件计划列表。
        depth: 文件嵌套目录深度（T10.5 基准用于模拟真实目录树）。

    Returns:
        与 ``files`` 表列对应的记录列表。
    """
    rows: list[FileRow] = []
    for idx, plan in enumerate(plans):
        rel = Path(_nested_dir(idx, depth)) / plan.file_name
        file_path = TESTDATA_ROOT / plan.spec.name / rel
        file_path.parent.mkdir(parents=True, exist_ok=True)
        content = random.Random().randbytes(plan.size)
        file_path.write_bytes(content)
        rows.append(
            FileRow(
                id=str(uuid.uuid4()),
                path=str(file_path),
                file_name=plan.file_name,
                file_size=plan.size,
                content_hash=hashlib.sha256(content).hexdigest(),
                category=plan.spec.name,
            )
        )
    return rows


def upsert_categories(conn: sqlite3.Connection) -> int:
    """写入（或刷新）内置分类。

    Args:
        conn: 数据库连接。

    Returns:
        写入的分类数量。
    """
    for sort_order, spec in enumerate(BUILTIN_CATEGORIES):
        conn.execute(
            UPSERT_CATEGORY_SQL,
            (str(uuid.uuid4()), spec.name, spec.icon, spec.color, sort_order),
        )
    return len(BUILTIN_CATEGORIES)


def upsert_files(conn: sqlite3.Connection, rows: list[FileRow]) -> None:
    """批量 upsert 文件记录（FTS 由触发器联动）。

    Args:
        conn: 数据库连接。
        rows: 文件记录列表。
    """
    conn.executemany(
        INSERT_FILE_SQL,
        [
            (r.id, r.path, r.file_name, r.file_size, r.content_hash, r.category)
            for r in rows
        ],
    )


def summarize(conn: sqlite3.Connection) -> None:
    """输出当前 ``files`` 表按分类的分布与 FTS 行数。

    Args:
        conn: 数据库连接。
    """
    total = conn.execute("SELECT count(*) FROM files WHERE is_deleted = 0").fetchone()[
        0
    ]
    fts_total = conn.execute("SELECT count(*) FROM file_fts").fetchone()[0]
    logger.info("files 表现存 %d 条（未删除），file_fts 现存 %d 行", total, fts_total)
    for name, count in conn.execute(
        "SELECT category, count(*) FROM files WHERE is_deleted = 0 GROUP BY category ORDER BY category"
    ):
        logger.info("  分类 %-4s %d 条", name, count)


def clean_testdata(conn: sqlite3.Connection | None, dry_run: bool) -> None:
    """删除测试目录与 ``files`` 表中对应记录。

    Args:
        conn: 数据库连接；数据库不存在时为 ``None``（仅清理目录）。
        dry_run: 为 ``True`` 时仅输出计划。
    """
    pattern = f"{TESTDATA_ROOT}{os.sep}%"
    if conn is not None:
        count = conn.execute(
            "SELECT count(*) FROM files WHERE path LIKE ?", (pattern,)
        ).fetchone()[0]
        logger.info("[clean] 将删除 files 表中测试记录 %d 条", count)
        if not dry_run:
            conn.execute("DELETE FROM files WHERE path LIKE ?", (pattern,))
    if TESTDATA_ROOT.exists():
        logger.info("[clean] 将删除目录 %s", TESTDATA_ROOT)
        if not dry_run:
            shutil.rmtree(TESTDATA_ROOT)
    else:
        logger.info("[clean] 测试目录不存在，跳过")
    if not dry_run and conn is not None:
        logger.info("[clean] 清理完成")


def run_generate(args: argparse.Namespace) -> int:
    """执行生成流程：校验参数 → 计划 → 落盘 → 入库 → 汇总。

    Args:
        args: 命令行参数。

    Returns:
        进程退出码。
    """
    if args.count < 1:
        logger.error("--count 必须 >= 1")
        return 1

    rng = random.Random(args.seed)
    plans = plan_files(rng, args.count)
    logger.info(
        "[plan] 目标目录 %s，数据库 %s，共 %d 个文件（seed=%d）",
        TESTDATA_ROOT,
        DB_PATH,
        len(plans),
        args.seed,
    )
    for spec in BUILTIN_CATEGORIES:
        n = sum(1 for p in plans if p.spec is spec)
        if n:
            logger.info("  %-4s %d 个", spec.name, n)
    if args.dry_run:
        logger.info("[dry-run] 仅输出计划，未写入")
        return 0

    if args.no_db:
        # T10.5 基准隔离：只落盘文件，不碰用户数据库（无 DB 前置条件）
        rows = materialize_files(plans, args.depth)
        logger.info(
            "[no-db] 已写入 %d 个文件（未入库，目录 %s）", len(rows), TESTDATA_ROOT
        )
        return 0

    try:
        conn = connect_db()
    except FileNotFoundError as e:
        logger.error("%s", e)
        return 1

    with conn:
        rows = materialize_files(plans, args.depth)
        category_count = upsert_categories(conn)
        upsert_files(conn, rows)
        logger.info("已写入 %d 个文件、%d 个内置分类", len(rows), category_count)
        summarize(conn)
    conn.close()
    return 0


def run_clean(args: argparse.Namespace) -> int:
    """执行清理流程。

    Args:
        args: 命令行参数。

    Returns:
        进程退出码。
    """
    conn: sqlite3.Connection | None = None
    if DB_PATH.exists():
        try:
            conn = connect_db()
        except FileNotFoundError as e:
            logger.warning("数据库不可用，仅清理目录：%s", e)
    else:
        logger.warning("数据库不存在，仅清理目录 %s", TESTDATA_ROOT)

    try:
        if conn is not None:
            with conn:
                clean_testdata(conn, args.dry_run)
        else:
            clean_testdata(None, args.dry_run)
    finally:
        if conn is not None:
            conn.close()
    return 0


def main(argv: list[str] | None = None) -> int:
    """脚本入口。

    Args:
        argv: 命令行参数，默认取 ``sys.argv[1:]``。

    Returns:
        进程退出码（0 成功，1 失败）。
    """
    global TESTDATA_ROOT, DB_PATH
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    args = parse_args(argv)
    # --root/--db：覆盖模块常量（run_generate/run_clean 均引用模块全局，重绑即生效）
    if args.root:
        TESTDATA_ROOT = Path(args.root).expanduser()
    if args.db:
        DB_PATH = Path(args.db).expanduser()
    if args.clean:
        return run_clean(args)
    return run_generate(args)


if __name__ == "__main__":
    sys.exit(main())
