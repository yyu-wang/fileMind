"""分类规则数据模型（08_Prompt工程设计 §7）。

规则 JSON 的 Pydantic 模型，供预置集校验（T4.1）与规则引擎（T4.2）共用。
``FileMeta`` 为匹配输入类型，供规则引擎与启发式层（T4.3）共用。
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal, Self

from pydantic import BaseModel, Field, model_validator

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(frozen=True)
class FileMeta:
    """待匹配文件元数据。``extension`` 约定为小写、不含点。"""

    name: str
    extension: str
    path: Path
    size: int


#: §7 condition.field 枚举
FIELD_VALUES = Literal["file_type", "file_name", "directory", "file_size"]
#: §7 condition.operator 枚举
OPERATOR_VALUES = Literal[
    "equals", "contains", "regex", "starts_with", "ends_with", "in", "gt", "lt"
]

_STR_OPERATORS = ("equals", "contains", "starts_with", "ends_with", "regex")
_NUMERIC_OPERATORS = ("gt", "lt")


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
        if self.operator in _STR_OPERATORS and not isinstance(self.value, str):
            raise ValueError(f"operator={self.operator} 时 value 必须为字符串")
        if self.operator in _NUMERIC_OPERATORS and not isinstance(self.value, (int, float)):
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
