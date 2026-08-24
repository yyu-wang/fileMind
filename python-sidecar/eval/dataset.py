"""T4.5 评估数据集生成与加载。

生成带正确标注的分类样本（JSONL），字段对齐 08_Prompt工程设计 §8 评估集规格：
``{"file_name", "file_type", "size", "content_summary", "correct_category",
"alternative_categories", "path", "modified_time"}``。

样本按 8 个评估分类（文档/图片/视频/音频/代码/压缩包/设计/其他）轮转生成，
并注入边界用例（无扩展名、日期命名、截图、超长文件名、emoji 等）。

注意：样本 ``path`` 含分类目录（``~/filemind-eval/<分类>/<年>/<季度>/``），
与产品目录信号一致（启发式层会读取目录），评估全程不真正落盘。
"""

from __future__ import annotations

import json
import random
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))  # noqa: E402
from gen_testdata import BUILTIN_CATEGORIES, NAME_SUFFIXES, CategorySpec  # noqa: E402

#: 默认评估集路径
DEFAULT_DATASET = Path.home() / ".filemind" / "eval" / "classify_eval.jsonl"

#: 8 个评估分类（与 gen_testdata 内置分类同名，作为混淆矩阵的行列）
CATEGORY_NAMES: tuple[str, ...] = tuple(spec.name for spec in BUILTIN_CATEGORIES)

#: 「其他」分类扩展名池：避开 .csv（会命中表格规则造成系统性误判）与
#: 预设规则已覆盖的扩展名，让该分类真正下沉到启发式扩展名映射/LLM 层。
_OTHER_EXTENSIONS: tuple[str, ...] = (".xyz", ".dat", ".log", ".bin", ".tmp", "")

#: 「其他」文件名词干池：避免与启发式关键词（如「备份」→压缩包）冲突
_OTHER_STEMS: tuple[str, ...] = (
    "临时文件",
    "unknown",
    "待整理",
    "notes",
    "随手记",
    "misc",
    "temp",
    "scratch",
    "junk",
    "cache_tmp",
)

#: 「其他」文件后缀池：排除 ``_v2``/``_v3``（触发版本号规则）与 ``-备份``
#: （触发「备份」→压缩包关键词），避免对兜底分类造成系统性误判。
_OTHER_SUFFIXES: tuple[str, ...] = ("", "-final", "-副本", "（新）", "-2024", "-draft")

#: 注入的边界用例：(文件名, 扩展名, 正确分类)
EDGE_CASES: tuple[tuple[str, str, str], ...] = (
    ("README", "", "文档"),
    ("LICENSE", "", "文档"),
    ("Dockerfile", "", "代码"),
    ("2024-08-15", "", "文档"),  # 日期命名 → 按日期归档规则
    ("Screenshot 2026-08-15 at 09.30", "", "图片"),  # 截图命名规则
    ("📊季度数据.xlsx", ".xlsx", "文档"),  # emoji 文件名
    ("报表（2026）·最终版.xlsx", ".xlsx", "文档"),  # 特殊字符 + 版本号词
    ("临时缓存_9f3a2c.bin", ".bin", "其他"),  # .bin → 数据文件 → 其他
    ("unknown_7k.xyz", ".xyz", "其他"),  # 未知扩展名 → LLM 层
    ("a" * 130 + ".txt", ".txt", "文档"),  # 超长文件名
)

_SIZE_UNITS: tuple[str, ...] = ("B", "KB", "MB", "GB", "TB")
_SIZE_SCALE = {"B": 1, "KB": 1024, "MB": 1024**2, "GB": 1024**3, "TB": 1024**4}


@dataclass(frozen=True)
class EvalRecord:
    """单个带标注样本（对齐 08-§8 评估集 JSONL 字段）。"""

    file_name: str
    file_type: str
    size: str
    content_summary: str
    correct_category: str
    alternative_categories: tuple[str, ...]
    path: str
    modified_time: str


def _eval_specs() -> tuple[CategorySpec, ...]:
    """BUILTIN_CATEGORIES 副本，替换「其他」的词干与扩展名池。"""
    return tuple(
        spec
        if spec.name != "其他"
        else CategorySpec(
            name=spec.name,
            icon=spec.icon,
            color=spec.color,
            stems=_OTHER_STEMS,
            extensions=_OTHER_EXTENSIONS,
            size_range=spec.size_range,
        )
        for spec in BUILTIN_CATEGORIES
    )


def _format_size(size: int) -> str:
    """字节数 → 人类可读大小（与 P-01 ``_human_size`` 一致，1 位小数）。"""
    value = float(size)
    unit = "B"
    for next_unit in _SIZE_UNITS[1:]:
        if abs(value) < 1024.0:
            break
        value /= 1024.0
        unit = next_unit
    if unit == "B":
        return f"{size} B"
    return f"{value:.1f} {unit}"


def parse_size(text: str) -> int:
    """人类可读大小（如 ``"2.3 MB"``）→ 字节数；解析失败返回 0。"""
    parts = text.strip().split(" ")
    if len(parts) != 2:
        return 0
    number, unit = parts
    scale = _SIZE_SCALE.get(unit.upper())
    try:
        value = float(number)
    except ValueError:
        return 0
    if scale is None:
        return 0
    return round(value * scale)


def _content_summary(stem: str, category: str) -> str:
    """按分类生成内容摘要（二进制类不提供可读文本）。"""
    if category in {"图片", "视频", "音频", "压缩包", "设计"}:
        return "（无文本内容，二进制文件。）"
    if category == "代码":
        return f"{stem}：源代码，含核心逻辑、注释与测试用例。"
    if category == "其他":
        return "临时文件，内容待整理，无明确分类信息。"
    return f"关于「{stem}」的工作文档，包含主要内容和待办事项。"


def _nested_path(category: str, index: int, file_name: str) -> str:
    """构造含分类目录的深层路径（供目录启发式与 LLM 上下文使用）。"""
    year = 2024 + (index % 3)
    quarter = (index % 4) + 1
    return str(Path.home() / "filemind-eval" / category / str(year) / f"Q{quarter}" / file_name)


def _alternatives(rng: random.Random, correct: str) -> tuple[str, ...]:
    """采样 1-2 个非正确分类作为候选分类。"""
    others = [c for c in CATEGORY_NAMES if c != correct]
    return tuple(rng.sample(others, rng.randint(1, min(2, len(others)))))


def _modified_time(rng: random.Random) -> str:
    """随机生成 ISO 时间戳（2026 年内）。"""
    month = rng.randint(1, 12)
    day = rng.randint(1, 28)
    hour = rng.randint(8, 18)
    minute = rng.randint(0, 59)
    return f"2026-{month:02d}-{day:02d}T{hour:02d}:{minute:02d}:00Z"


def _build_name(
    rng: random.Random, spec: CategorySpec, used: set[str], suffixes: tuple[str, ...]
) -> str:
    """生成目录内唯一的文件名（词干 + 后缀 + 扩展名）。"""
    while True:
        stem = rng.choice(spec.stems)
        suffix = rng.choice(suffixes)
        ext = rng.choice(spec.extensions)
        name = f"{stem}{suffix}{ext}"
        if name not in used:
            used.add(name)
            return name


def _record(
    rng: random.Random,
    index: int,
    name: str,
    category: str,
    file_type: str,
    size: int,
) -> EvalRecord:
    """聚合各字段构造 EvalRecord。"""
    return EvalRecord(
        file_name=name,
        file_type=file_type,
        size=_format_size(size),
        content_summary=_content_summary(Path(name).stem, category),
        correct_category=category,
        alternative_categories=_alternatives(rng, category),
        path=_nested_path(category, index, name),
        modified_time=_modified_time(rng),
    )


def generate_dataset(count: int = 500, seed: int = 42) -> list[EvalRecord]:
    """生成 ``count`` 个带标注样本（分类轮转 + 边界用例）。

    Args:
        count: 样本总数（默认 500），需大于边界用例数。
        seed: 随机种子（默认 42），保证可复现。

    Returns:
        样本列表，分类近似均匀分布。
    """
    if count < len(EDGE_CASES):
        raise ValueError(f"count 必须 >= 边界用例数 {len(EDGE_CASES)}")
    rng = random.Random(seed)
    specs = _eval_specs()
    spec_by_name = {spec.name: spec for spec in specs}
    used: set[str] = set()
    records: list[EvalRecord] = []

    regular = count - len(EDGE_CASES)
    for index in range(regular):
        spec = specs[index % len(specs)]
        suffixes = _OTHER_SUFFIXES if spec.name == "其他" else NAME_SUFFIXES
        name = _build_name(rng, spec, used, suffixes)
        ext = name[name.rfind(".") :] if "." in name else ""
        size = rng.randrange(spec.size_range[0], spec.size_range[1] + 1)
        records.append(_record(rng, index, name, spec.name, ext, size))

    for offset, (name, file_type, category) in enumerate(EDGE_CASES):
        if name in used:
            continue
        used.add(name)
        spec = spec_by_name[category]
        size = rng.randrange(spec.size_range[0], spec.size_range[1] + 1)
        records.append(_record(rng, regular + offset, name, category, file_type, size))

    return records


def write_jsonl(path: Path, records: list[EvalRecord]) -> None:
    """将样本集写入 JSONL（每行一个 JSON 对象）。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for record in records:
            fh.write(json.dumps(asdict(record), ensure_ascii=False) + "\n")


def load_jsonl(path: Path) -> list[EvalRecord]:
    """从 JSONL 加载样本集；缺失的可选字段以空值兜底。"""
    records: list[EvalRecord] = []
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            data = json.loads(line)
            records.append(
                EvalRecord(
                    file_name=str(data["file_name"]),
                    file_type=str(data.get("file_type", "")),
                    size=str(data.get("size", "0 B")),
                    content_summary=str(data.get("content_summary", "")),
                    correct_category=str(data["correct_category"]),
                    alternative_categories=tuple(
                        str(c) for c in data.get("alternative_categories", [])
                    ),
                    path=str(data.get("path", "")),
                    modified_time=str(data.get("modified_time", "")),
                )
            )
    return records
