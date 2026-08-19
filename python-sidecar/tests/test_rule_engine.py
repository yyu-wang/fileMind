"""T4.2 — 分类规则引擎单元测试。

覆盖：预置集加载、三类规则命中、优先级短路、同优先级 id 升序、
全部 operator、禁用规则过滤、热更新。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.engine import RuleEngine, RuleMatch  # noqa: E402
from app.rules.models import FileMeta  # noqa: E402

PRESET_RULES_PATH = (
    Path(__file__).resolve().parents[1] / "app" / "rules" / "presets" / "preset_rules.json"
)


def make_engine() -> RuleEngine:
    """加载真实预置集的引擎。"""
    engine = RuleEngine(PRESET_RULES_PATH)
    engine.load()
    return engine


def make_file(
    name: str = "report.pdf",
    size: int = 1000,
    parent: str = "/data",
) -> FileMeta:
    """构造测试文件（extension 自动取小写无点）。"""
    return FileMeta(
        name=name,
        extension=Path(name).suffix.lstrip(".").lower(),
        path=Path(parent) / name,
        size=size,
    )


def write_preset(tmp_path: Path, rules: list[dict[str, object]], version: int = 1) -> Path:
    """写入临时预置规则 JSON 并返回路径。"""
    path = tmp_path / "preset_rules.json"
    path.write_text(
        json.dumps({"version": version, "description": "test", "rules": rules}, ensure_ascii=False),
        encoding="utf-8",
    )
    return path


def test_loads_13_preset_rules() -> None:
    """真实预置集加载 13 条，按优先级降序排列。"""
    engine = make_engine()
    assert len(engine.rules) == 13
    assert engine.version == 1
    priorities = [rule.priority for rule in engine.rules]
    assert priorities == sorted(priorities, reverse=True)


def test_match_doc_file() -> None:
    """file_type 规则：report.pdf → 文档。"""
    match = make_engine().match_file(make_file("report.pdf"))
    assert match is not None
    assert match.rule_id == "preset_doc_001"
    assert match.category == "文档"


def test_match_image_file() -> None:
    """file_type 规则：photo.png → 图片。"""
    match = make_engine().match_file(make_file("photo.png"))
    assert match is not None
    assert match.category == "图片"


def test_match_date_file_with_unknown_ext() -> None:
    """file_name 日期规则：未知扩展名不干扰日期命中。"""
    match = make_engine().match_file(make_file("2024-08-15_notes.bkp"))
    assert match is not None
    assert match.rule_id == "preset_name_date_001"
    assert match.category == "按日期归档"
    assert match.sub_category == "{{year}}年{{month}}月"


def test_match_screenshot_with_unknown_ext() -> None:
    """file_name 截图规则：截图_ 前缀命中。"""
    match = make_engine().match_file(make_file("截图_20240815_notes.xyz"))
    assert match is not None
    assert match.rule_id == "preset_name_screenshot_001"
    assert match.category == "截图"


def test_match_project_directory() -> None:
    """directory 规则：/projects/ 下未知扩展名文件 → 项目文件。"""
    file = make_file("notes.bkp", parent="/Users/me/projects/myapp")
    match = make_engine().match_file(file)
    assert match is not None
    assert match.rule_id == "preset_dir_project_001"
    assert match.category == "项目文件"


def test_no_match_returns_none() -> None:
    """无规则命中 → None。"""
    match = make_engine().match_file(make_file("weird.zzzz", parent="/tmp"))
    assert match is None


def test_priority_short_circuit(tmp_path: Path) -> None:
    """高优先级先命中，短路跳过低优先级规则。"""
    rules = [
        {
            "id": "r_low",
            "name": "低",
            "priority": 50,
            "enabled": True,
            "condition": {"field": "file_name", "operator": "contains", "value": "report"},
            "action": {"category": "低分类"},
        },
        {
            "id": "r_high",
            "name": "高",
            "priority": 90,
            "enabled": True,
            "condition": {"field": "file_name", "operator": "contains", "value": "report"},
            "action": {"category": "高分类"},
        },
    ]
    engine = RuleEngine(write_preset(tmp_path, rules))
    engine.load()
    match = engine.match_file(make_file("report.pdf"))
    assert match is not None
    assert match.rule_id == "r_high"
    assert match.category == "高分类"


def test_same_priority_id_asc_tiebreak(tmp_path: Path) -> None:
    """同优先级按 id 升序取第一条（确定性）。"""
    rules = [
        {
            "id": "r_b",
            "name": "B",
            "priority": 90,
            "enabled": True,
            "condition": {"field": "file_name", "operator": "contains", "value": "report"},
            "action": {"category": "B 分类"},
        },
        {
            "id": "r_a",
            "name": "A",
            "priority": 90,
            "enabled": True,
            "condition": {"field": "file_name", "operator": "contains", "value": "report"},
            "action": {"category": "A 分类"},
        },
    ]
    engine = RuleEngine(write_preset(tmp_path, rules))
    engine.load()
    match = engine.match_file(make_file("report.pdf"))
    assert match is not None
    assert match.rule_id == "r_a"
    assert match.category == "A 分类"


@pytest.mark.parametrize(
    ("operator", "field", "value", "file", "expect"),
    [
        ("equals", "file_type", "pdf", make_file("report.pdf"), True),
        ("equals", "file_type", "pdf", make_file("report.docx"), False),
        ("starts_with", "file_name", "财务", make_file("财务报销.xlsx"), True),
        ("starts_with", "file_name", "财务", make_file("非财务报销.xlsx"), False),
        ("ends_with", "file_name", ".pdf", make_file("report.pdf"), True),
        ("ends_with", "file_name", ".pdf", make_file("report.docx"), False),
        ("gt", "file_size", 1024, make_file("big.pdf", size=2048), True),
        ("gt", "file_size", 1024, make_file("small.pdf", size=512), False),
        ("lt", "file_size", 1024, make_file("small.pdf", size=512), True),
        ("lt", "file_size", 1024, make_file("big.pdf", size=2048), False),
    ],
    ids=[
        "equals-hit",
        "equals-miss",
        "starts_hit",
        "starts_miss",
        "ends_hit",
        "ends_miss",
        "gt-hit",
        "gt-miss",
        "lt-hit",
        "lt-miss",
    ],
)
def test_operators(
    tmp_path: Path,
    operator: str,
    field: str,
    value: object,
    file: FileMeta,
    expect: bool,
) -> None:
    """非预置 operator（equals/starts_with/ends_with/gt/lt）行为正确。"""
    rules = [
        {
            "id": "r1",
            "name": "op",
            "priority": 90,
            "enabled": True,
            "condition": {"field": field, "operator": operator, "value": value},
            "action": {"category": "分类"},
        }
    ]
    engine = RuleEngine(write_preset(tmp_path, rules))
    engine.load()
    match = engine.match_file(file)
    assert (match is not None) == expect


def test_disabled_rule_not_matched(tmp_path: Path) -> None:
    """enabled=false 的规则不参与匹配。"""
    rules = [
        {
            "id": "r_disabled",
            "name": "禁用",
            "priority": 90,
            "enabled": False,
            "condition": {"field": "file_name", "operator": "contains", "value": "report"},
            "action": {"category": "禁用分类"},
        }
    ]
    engine = RuleEngine(write_preset(tmp_path, rules))
    engine.load()
    assert engine.rules == ()
    assert engine.match_file(make_file("report.pdf")) is None


def test_reload_hot_update(tmp_path: Path) -> None:
    """reload 后新规则生效（规则热更新）。"""
    path = write_preset(
        tmp_path,
        [
            {
                "id": "r1",
                "name": "旧",
                "priority": 90,
                "enabled": True,
                "condition": {"field": "file_name", "operator": "contains", "value": "report"},
                "action": {"category": "旧分类"},
            }
        ],
    )
    engine = RuleEngine(path)
    engine.load()
    match_before = engine.match_file(make_file("report.pdf"))
    assert match_before is not None
    assert match_before.category == "旧分类"

    path.write_text(
        json.dumps(
            {
                "version": 1,
                "description": "test",
                "rules": [
                    {
                        "id": "r2",
                        "name": "新",
                        "priority": 90,
                        "enabled": True,
                        "condition": {
                            "field": "file_name",
                            "operator": "contains",
                            "value": "report",
                        },
                        "action": {"category": "新分类"},
                    }
                ],
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    engine.reload()
    match = engine.match_file(make_file("report.pdf"))
    assert match is not None
    assert match.category == "新分类"
    assert match.rule_id == "r2"


def test_reload_if_changed_detects_mtime(tmp_path: Path) -> None:
    """reload_if_changed：文件未变不重载，变化后重载。"""
    path = write_preset(
        tmp_path,
        [
            {
                "id": "r1",
                "name": "原",
                "priority": 90,
                "enabled": True,
                "condition": {"field": "file_name", "operator": "contains", "value": "report"},
                "action": {"category": "原分类"},
            }
        ],
    )
    engine = RuleEngine(path)
    engine.load()

    assert engine.reload_if_changed() is False
    match_before = engine.match_file(make_file("report.pdf"))
    assert match_before is not None
    assert match_before.category == "原分类"

    path.write_text(
        json.dumps(
            {
                "version": 1,
                "description": "test",
                "rules": [
                    {
                        "id": "r2",
                        "name": "改",
                        "priority": 90,
                        "enabled": True,
                        "condition": {
                            "field": "file_name",
                            "operator": "contains",
                            "value": "report",
                        },
                        "action": {"category": "改分类"},
                    }
                ],
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    assert engine.reload_if_changed() is True
    match = engine.match_file(make_file("report.pdf"))
    assert match is not None
    assert match.category == "改分类"


def test_rule_match_is_dataclass() -> None:
    """命中结果为 RuleMatch，字段齐全。"""
    match = make_engine().match_file(make_file("report.pdf"))
    assert isinstance(match, RuleMatch)
    assert match.rule_id and match.rule_name and match.category
