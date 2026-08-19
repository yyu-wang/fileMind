"""T4.1 — 分类规则预置集 JSON 校验测试。

规则格式与内容来源：08_Prompt工程设计 §7（field/operator/value + action + priority）。
本测试用 Pydantic 模型做结构校验，不引入 jsonschema 依赖。
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Literal, Self

import pytest
from pydantic import BaseModel, Field, ValidationError, model_validator

PRESET_RULES_PATH = (
    Path(__file__).resolve().parents[1] / "app" / "rules" / "presets" / "preset_rules.json"
)

FIELD_VALUES = Literal["file_type", "file_name", "directory", "file_size"]
OPERATOR_VALUES = Literal[
    "equals", "contains", "regex", "starts_with", "ends_with", "in", "gt", "lt"
]
STR_OPERATORS = ("equals", "contains", "starts_with", "ends_with", "regex")
NUMERIC_OPERATORS = ("gt", "lt")


class Condition(BaseModel):
    """§7 condition：field + operator + value。"""

    field: FIELD_VALUES
    operator: OPERATOR_VALUES
    value: str | list[str] | int | float

    @model_validator(mode="after")
    def check_value_matches_operator(self) -> Self:
        """operator 与 value 类型必须匹配（in→非空数组 / 文本→str / 数值→gt,lt）。"""
        if self.operator == "in" and (not isinstance(self.value, list) or not self.value):
            raise ValueError("operator=in 时 value 必须为非空数组")
        if self.operator in STR_OPERATORS and not isinstance(self.value, str):
            raise ValueError(f"operator={self.operator} 时 value 必须为字符串")
        if self.operator in NUMERIC_OPERATORS and not isinstance(self.value, (int, float)):
            raise ValueError(f"operator={self.operator} 时 value 必须为数值")
        return self


class Action(BaseModel):
    """§7 action：分类 + 可选子分类（支持 {{year}} 等变量模板）。"""

    category: str = Field(min_length=1)
    sub_category: str | None = None


class PresetRule(BaseModel):
    """§7 单条规则。"""

    id: str
    name: str
    priority: int
    enabled: bool
    condition: Condition
    action: Action


class PresetRules(BaseModel):
    """预置规则集顶层结构。"""

    version: int
    description: str
    rules: list[PresetRule]


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
    assert date_rule.priority == 70
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
