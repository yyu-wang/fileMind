"""T4.1 — 分类规则预置集 JSON 校验测试。

规则格式与内容来源：08_Prompt工程设计 §7（field/operator/value + action + priority）。
本测试复用 app.rules.models 的 Pydantic 模型做结构校验。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest
from pydantic import ValidationError

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.models import PresetRule, PresetRules  # noqa: E402

PRESET_RULES_PATH = (
    Path(__file__).resolve().parents[1] / "app" / "rules" / "presets" / "preset_rules.json"
)


def load_preset_rules() -> PresetRules:
    """加载并校验预置规则 JSON 文件。"""
    raw = json.loads(PRESET_RULES_PATH.read_text(encoding="utf-8"))
    return PresetRules.model_validate(raw)


def test_preset_rules_loadable_and_versioned() -> None:
    """JSON 可解析、顶层含版本号且 rules 非空。"""
    rules = load_preset_rules()
    assert rules.version == 1
    assert len(rules.rules) > 0
    assert rules.description


def test_rule_ids_unique() -> None:
    """规则 ID 全局唯一（确定性匹配前提）。"""
    rules = load_preset_rules()
    ids = [rule.id for rule in rules.rules]
    assert len(ids) == len(set(ids))


def test_priority_within_preset_range() -> None:
    """预置规则优先级 ∈ [60, 100]（用户自定义规则才用 1-50）。"""
    rules = load_preset_rules()
    for rule in rules.rules:
        assert 60 <= rule.priority <= 100, f"{rule.id} 优先级越界: {rule.priority}"


def test_action_category_non_empty() -> None:
    """每条规则的 action.category 非空。"""
    rules = load_preset_rules()
    for rule in rules.rules:
        assert rule.action.category, f"{rule.id} action.category 为空"


def test_rule_type_count_matches_section7() -> None:
    """规则总数 13：file_type×9 / file_name×3 / directory×1（对齐 08-§7）。"""
    rules = load_preset_rules()
    counts: dict[str, int] = {}
    for rule in rules.rules:
        counts[rule.condition.field] = counts.get(rule.condition.field, 0) + 1
    assert len(rules.rules) == 13
    assert counts == {"file_type": 9, "file_name": 3, "directory": 1}


def test_file_type_rules_use_in_operator() -> None:
    """file_type 规则统一用 `in` 操作符（扩展名白名单）。"""
    rules = load_preset_rules()
    for rule in rules.rules:
        if rule.condition.field == "file_type":
            assert rule.condition.operator == "in"
            assert isinstance(rule.condition.value, list)
            assert len(rule.condition.value) > 0


def test_section7_spot_rules_present() -> None:
    """抽查 §7 关键规则：文档扩展名、日期正则、项目目录正则。"""
    rules = load_preset_rules()
    by_id = {rule.id: rule for rule in rules.rules}

    doc = by_id["preset_doc_001"]
    assert doc.priority == 90
    assert doc.condition.value == ["pdf", "doc", "docx", "txt", "md", "rtf", "odt"]
    assert doc.action.category == "文档"

    date_rule = by_id["preset_name_date_001"]
    # SC-M6：文件名信号强于扩展名（日期 95 > 扩展名 90），否则死规则
    assert date_rule.priority == 95
    assert date_rule.condition.operator == "regex"
    assert date_rule.condition.value == r"^(20\d{2})[-_]?(0[1-9]|1[0-2])[-_]?(0[1-9]|[12]\d|3[01])"
    assert date_rule.action.sub_category == "{{year}}年{{month}}月"

    project = by_id["preset_dir_project_001"]
    assert project.priority == 60
    assert project.action.category == "项目文件"


@pytest.mark.parametrize(
    "payload",
    [
        {"id": "x", "name": "n", "priority": 80, "enabled": True},
        {
            "id": "x",
            "name": "n",
            "priority": 80,
            "enabled": True,
            "condition": {"field": "file_type", "operator": "in", "value": []},
            "action": {"category": "文档"},
        },
        {
            "id": "x",
            "name": "n",
            "priority": 80,
            "enabled": True,
            "condition": {"field": "file_size", "operator": "gt", "value": "abc"},
            "action": {"category": "文档"},
        },
    ],
    ids=["缺 condition/action", "in 空数组", "gt 要求数值"],
)
def test_malformed_rule_rejected(payload: dict[str, object]) -> None:
    """结构/取值不合规的规则必须被 Pydantic 拒绝。"""
    with pytest.raises(ValidationError):
        PresetRule.model_validate(payload)


# ---------- SC-M6：文件名规则优先级高于扩展名（端到端回归） ----------


def test_filename_rules_beat_extension_rules() -> None:
    """SC-M6：文件名特异规则（日期/截图/版本）必须先于扩展名规则命中。

    修复前扩展名规则 priority=90 高于文件名规则（70/65），任何有已知
    扩展名的文件永远先命中扩展名 → 文件名规则是死规则。
    """
    from pathlib import Path

    from app.rules.engine import RuleEngine
    from app.rules.models import FileMeta

    engine = RuleEngine(
        Path(__file__).resolve().parents[1] / "app" / "rules" / "presets" / "preset_rules.json"
    )
    engine.load()

    def match(name: str) -> str | None:
        m = engine.match_file(
            FileMeta(
                name=name,
                extension=Path(name).suffix.lstrip(".").lower(),
                path=Path("/data") / name,
                size=1024,
            )
        )
        return m.category if m else None

    # Screenshot.png：旧正则字符类缺 '.' → 不命中 → 落入扩展名「图片」
    assert match("Screenshot.png") == "截图"
    assert match("截图 2024-08-15.png") == "截图"
    assert match("20240101_报告.pdf") == "按日期归档"
    # 版本后缀是弱信号：已知类型让位于扩展名（eval 数据集以此为 ground truth）
    assert match("报告_v2.pdf") == "文档"
    # 无扩展名的版本文件才由版本规则兜底
    assert match("设计稿_最终版") == "版本文件"
    # 普通文档：文件名规则不命中 → 扩展名兜底不受影响
    assert match("报告.pdf") == "文档"
