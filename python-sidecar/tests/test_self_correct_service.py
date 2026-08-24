"""T5.7 — 自我纠正服务单元测试（P-04 提示词 + JSON 解析 + 验证编排）。

覆盖：P-04 提示词构建（SYSTEM 4 检查项 + 严格 JSON；USER 含 query/context/answer/
Few-shot/[待检查]）、``parse_self_correct_response`` 各分支（合法 correct / incorrect
带 issues+corrected / 畸形→fail-open / corrected 空串→None / markdown 包裹 / 空 issues
默认 reason）、``validate_answer``（mock call_ollama_json 成功 / 超时→fail-open /
LLM 不可用冒泡）。均不发真实推理。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.self_correct_service import (  # noqa: E402
    SelfCorrectResult,
    build_self_correct_prompt,
    parse_self_correct_response,
    validate_answer,
)

# ------------------------------------------------------------------
# build_self_correct_prompt（P-04）
# ------------------------------------------------------------------


def test_system_has_check_items_and_json_constraint() -> None:
    """SYSTEM 含 4 条检查项与严格 JSON 输出约束。"""
    system, _ = build_self_correct_prompt("查询", "片段", "回答")
    assert "幻觉检测" in system
    assert "引用正确性" in system
    assert "数据准确性" in system
    assert "完整性" in system
    assert "严格 JSON" in system
    assert "只输出 JSON" in system


def test_user_has_query_context_answer_fewshot() -> None:
    """USER 含原始问题、文档片段、待检查回答、Few-shot 与 [待检查]。"""
    _, user = build_self_correct_prompt(
        "2024年Q3营收多少", "[1] 营收为5.2亿元", "营收为5.5亿元 [1]"
    )
    assert "## 原始问题\n2024年Q3营收多少" in user
    assert "## 文档片段（事实依据）\n[1] 营收为5.2亿元" in user
    assert "## 待检查的回答\n营收为5.5亿元 [1]" in user
    assert "示例 1" in user and "示例 3" in user
    assert "[待检查]" in user


# ------------------------------------------------------------------
# parse_self_correct_response
# ------------------------------------------------------------------


def test_parse_correct() -> None:
    """is_correct=true → 无 issues、corrected_answer 为 None。"""
    result = parse_self_correct_response(
        '{"is_correct": true, "issues": [], "corrected_answer": null}'
    )
    assert result.is_correct is True
    assert result.issues == []
    assert result.corrected_answer is None


def test_parse_incorrect_with_issues_and_corrected() -> None:
    """is_correct=false → 保留 issues 与 corrected_answer，reason 为 issues 拼接。"""
    result = parse_self_correct_response(
        '{"is_correct": false, "issues": ["数据错误：文档片段中营收为5.2亿，回答中为5.5亿"], '
        '"corrected_answer": "营收为 5.2 亿元 [1]"}'
    )
    assert result.is_correct is False
    assert result.corrected_answer == "营收为 5.2 亿元 [1]"
    assert "数据错误" in result.reason


def test_parse_incorrect_empty_issues_uses_default_reason() -> None:
    """is_correct=false 但 issues 为空 → 默认 reason「答案无引用支撑」。"""
    result = parse_self_correct_response(
        '{"is_correct": false, "issues": [], "corrected_answer": "修正"}'
    )
    assert result.is_correct is False
    assert result.reason == "答案无引用支撑"


def test_parse_malformed_fail_open() -> None:
    """畸形 JSON → is_correct=true（fail-open，不打断已生成回答）。"""
    result = parse_self_correct_response("{not valid json")
    assert result.is_correct is True
    assert "解析失败" in result.reason


def test_parse_fenced_json_code_block() -> None:
    """markdown 代码块包裹 JSON → 仍能解析。"""
    result = parse_self_correct_response(
        '```json\n{"is_correct": true, "issues": [], "corrected_answer": null}\n```'
    )
    assert result.is_correct is True


def test_parse_empty_corrected_answer_to_none() -> None:
    """corrected_answer 空串 → 归一为 None（无可交付的修正回答）。"""
    result = parse_self_correct_response(
        '{"is_correct": false, "issues": ["x"], "corrected_answer": ""}'
    )
    assert result.corrected_answer is None


# ------------------------------------------------------------------
# validate_answer（mock call_ollama_json）
# ------------------------------------------------------------------


async def test_validate_success() -> None:
    """正常 LLM 返回 → 解析为 SelfCorrectResult。"""

    async def fake_call(system: str, user: str) -> str:
        return '{"is_correct": true, "issues": [], "corrected_answer": null}'

    with mock.patch("app.services.self_correct_service.call_ollama_json", fake_call):
        result = await validate_answer("查询", "片段", "回答")

    assert isinstance(result, SelfCorrectResult)
    assert result.is_correct is True


async def test_validate_timeout_fail_open() -> None:
    """LLM 超时 → is_correct=true（fail-open），reason 标记超时。"""

    async def fake_call(system: str, user: str) -> str:
        raise TimeoutError

    with mock.patch("app.services.self_correct_service.call_ollama_json", fake_call):
        result = await validate_answer("查询", "片段", "回答")

    assert result.is_correct is True
    assert "超时" in result.reason


async def test_validate_llm_unavailable_propagates() -> None:
    """Ollama 不可用 → LLMUnavailableError 冒泡给路由（捕获后跳过纠正）。"""

    async def fake_call(system: str, user: str) -> str:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch("app.services.self_correct_service.call_ollama_json", fake_call),
        pytest.raises(LLMUnavailableError),
    ):
        await validate_answer("查询", "片段", "回答")


# ------------------------------------------------------------------
# T8.5 云端变体（Prompt 版本适配）
# ------------------------------------------------------------------

#: P-04 JSON schema 字段（local/cloud 必须完全一致，DoD 依据）
_SELF_CORRECT_SCHEMA_FIELDS = ("is_correct", "issues", "corrected_answer")


def test_build_prompt_cloud_schema_consistent_with_local() -> None:
    """云端变体 JSON schema 字段与本地完全一致；few-shot 3 → 1。"""
    local_system, local_user = build_self_correct_prompt("查询", "片段", "回答")
    cloud_system, cloud_user = build_self_correct_prompt("查询", "片段", "回答", version="cloud")
    for field in _SELF_CORRECT_SCHEMA_FIELDS:
        assert field in local_system, field
        assert field in cloud_system, field
    assert cloud_user.count("[示例") == 1
    assert local_user.count("[示例") == 3


async def test_validate_answer_cloud_provider_uses_generate() -> None:
    """传入云端 Provider → 走 provider.generate(json_mode=True) 并解析。"""
    from app.services.cloud_provider import CloudUnavailableError

    class FakeCloud:
        version = "cloud"
        calls: list[dict[str, object]] = []

        async def generate(
            self,
            system: str,
            user: str,
            *,
            temperature: float = 0.2,
            max_tokens: int | None = None,
            json_mode: bool = False,
            **kwargs: object,
        ) -> str:
            FakeCloud.calls.append(
                {"json_mode": json_mode, "temperature": temperature, "max_tokens": max_tokens}
            )
            return '{"is_correct": true, "issues": [], "corrected_answer": null}'

    result = await validate_answer("查询", "片段", "回答", provider=FakeCloud())  # type: ignore[arg-type]
    assert result.is_correct is True
    assert len(FakeCloud.calls) == 1
    assert FakeCloud.calls[0]["json_mode"] is True
    assert FakeCloud.calls[0]["temperature"] == 0.0
    assert FakeCloud.calls[0]["max_tokens"] == 256

    class BoomCloud:
        version = "cloud"

        async def generate(self, *args: object, **kwargs: object) -> str:
            raise CloudUnavailableError("proxy down")

    with pytest.raises(LLMUnavailableError):
        await validate_answer("查询", "片段", "回答", provider=BoomCloud())  # type: ignore[arg-type]
