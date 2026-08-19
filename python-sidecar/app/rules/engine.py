"""分类规则引擎：JSON 加载、优先级匹配、短路执行与热更新。

规则格式与冲突处理来源：08_Prompt工程设计 §7。
规则按 priority DESC + id ASC 排序，首个命中即返回（短路执行）。
匹配输入 ``FileMeta`` 对齐 API 规格书 §3.2 files 元素。
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

from app.rules.models import Condition, FileMeta, PresetRule, PresetRules

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

_FieldValue = str | int


@dataclass(frozen=True)
class RuleMatch:
    """单条规则命中结果。"""

    rule_id: str
    rule_name: str
    category: str
    sub_category: str | None


def _get_file_type(file: FileMeta) -> str:
    return file.extension


def _get_file_name(file: FileMeta) -> str:
    return file.name


def _get_directory(file: FileMeta) -> str:
    return str(file.path.parent)


def _get_file_size(file: FileMeta) -> int:
    return file.size


#: field → FileMeta 取值函数映射（§7 condition.field）
_FIELD_GETTERS: dict[str, Callable[[FileMeta], _FieldValue]] = {
    "file_type": _get_file_type,
    "file_name": _get_file_name,
    "directory": _get_directory,
    "file_size": _get_file_size,
}


def _matches(condition: Condition, file: FileMeta) -> bool:
    """判断单条规则的 condition 是否命中文件。"""
    field_value = _FIELD_GETTERS[condition.field](file)
    value = condition.value

    if condition.operator == "in":
        return isinstance(value, list) and isinstance(field_value, str) and field_value in value
    if condition.operator == "equals":
        return field_value == value
    if condition.operator == "contains":
        return isinstance(field_value, str) and isinstance(value, str) and value in field_value
    if condition.operator == "starts_with":
        return (
            isinstance(field_value, str)
            and isinstance(value, str)
            and field_value.startswith(value)
        )
    if condition.operator == "ends_with":
        return (
            isinstance(field_value, str) and isinstance(value, str) and field_value.endswith(value)
        )
    if condition.operator == "regex":
        return (
            isinstance(field_value, str)
            and isinstance(value, str)
            and re.search(value, field_value) is not None
        )
    if condition.operator == "gt":
        return (
            isinstance(field_value, (int, float))
            and isinstance(value, (int, float))
            and field_value > value
        )
    if condition.operator == "lt":
        return (
            isinstance(field_value, (int, float))
            and isinstance(value, (int, float))
            and field_value < value
        )
    return False


class RuleEngine:
    """加载预置规则并按优先级短路匹配的规则引擎。

    线程安全说明：预置规则在加载时整体替换（``_rules`` 为不可变 tuple），
    匹配过程不修改状态，可安全被并发读取。
    """

    def __init__(self, presets_path: Path) -> None:
        self._presets_path = presets_path
        self._rules: tuple[PresetRule, ...] = ()
        self._version: int | None = None
        self._mtime_ns: int | None = None

    @property
    def rules(self) -> tuple[PresetRule, ...]:
        """已加载的启用规则（priority DESC + id ASC）。"""
        return self._rules

    @property
    def version(self) -> int | None:
        """预置集版本号。"""
        return self._version

    def load(self) -> None:
        """从 JSON 加载并排序。"""
        presets = PresetRules.model_validate(
            json.loads(self._presets_path.read_text(encoding="utf-8"))
        )
        self._version = presets.version
        self._rules = tuple(
            sorted(
                (rule for rule in presets.rules if rule.enabled),
                key=lambda rule: (-rule.priority, rule.id),
            )
        )
        self._mtime_ns = self._presets_path.stat().st_mtime_ns

    def reload(self) -> None:
        """强制重载（规则热更新入口）。"""
        self.load()

    def reload_if_changed(self) -> bool:
        """文件 mtime 变化时重载，返回是否发生了重载。"""
        current_mtime = self._presets_path.stat().st_mtime_ns
        if self._mtime_ns is not None and current_mtime == self._mtime_ns:
            return False
        self.load()
        return True

    def match_file(self, file: FileMeta) -> RuleMatch | None:
        """按优先级短路匹配，返回首个命中规则结果；无命中返回 None。"""
        for rule in self._rules:
            if _matches(rule.condition, file):
                return RuleMatch(
                    rule_id=rule.id,
                    rule_name=rule.name,
                    category=rule.action.category,
                    sub_category=rule.action.sub_category,
                )
        return None
