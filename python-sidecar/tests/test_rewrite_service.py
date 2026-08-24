"""T5.5 — 查询改写服务单元测试。

覆盖：P-02 提示词构建、历史格式化（取最近 3 轮）、JSON 解析兜底、
rewrite_query 各分支（无历史短路 / 成功 / 超时 / LLM 不可用 / 解析失败）。
均 mock LLM 调用，不发真实推理。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.rules.llm_classify import LLMUnavailableError  # noqa: E402
from app.services.rewrite_service import (  # noqa: E402
    HISTORY_TURNS,
    ConversationTurn,
    RewriteResult,
    build_rewrite_prompt,
    format_history,
    parse_rewrite_response,
    rewrite_query,
)


def turn(user: str = "用户问", assistant: str = "助手答") -> ConversationTurn:
    """构造一轮对话。"""
    return ConversationTurn(user=user, assistant=assistant)


# ------------------------------------------------------------------
# format_history
# ------------------------------------------------------------------


def test_format_history_empty() -> None:
    """空历史 → 空串。"""
    assert format_history([]) == ""


def test_format_history_single_turn() -> None:
    """单轮 → "用户：...\n助手：..."。"""
    assert (
        format_history([turn("怎么整理照片？", "按日期分类")])
        == "用户：怎么整理照片？\n助手：按日期分类"
    )


def test_format_history_takes_last_three() -> None:
    """5 轮 → 只取最近 3 轮（跳过最旧 2 轮）。"""
    turns = [turn(f"q{i}", f"a{i}") for i in range(5)]
    text = format_history(turns)
    assert text.count("用户：") == HISTORY_TURNS
    assert "q0" not in text and "q1" not in text
    assert "q2" in text and "q3" in text and "q4" in text
    # 多轮之间空行分隔
    assert "\n\n" in text


# ------------------------------------------------------------------
# build_rewrite_prompt
# ------------------------------------------------------------------


def test_build_prompt_system_has_rules() -> None:
    """SYSTEM 含改写规则与严格 JSON 约束。"""
    system, _ = build_rewrite_prompt("查询", [])
    assert "改写规则" in system
    assert "严格 JSON" in system
    assert "代词" in system


def test_build_prompt_user_has_history_fewshot_and_query() -> None:
    """USER 含格式化历史（出现两处）、Few-shot 示例、[待改写] 与查询。"""
    # 用唯一文本避免与 Few-shot 示例 1（同为营收文本）混淆计数
    history = [turn("自定义历史问题", "自定义历史回答")]
    _, user = build_rewrite_prompt("那利润呢？", history)
    assert user.count("自定义历史问题") == 2  # 历史按规格出现两次
    assert "示例 1" in user and "示例 3" in user
    assert "[待改写]" in user
    assert "用户查询：那利润呢？" in user


# ------------------------------------------------------------------
# parse_rewrite_response
# ------------------------------------------------------------------


def test_parse_valid_json() -> None:
    """合法 JSON → 字段解析正确。"""
    result = parse_rewrite_response(
        '{"rewritten_query": "2024年Q3的利润是多少", "need_rewrite": true, '
        '"expanded_keywords": ["2024", "Q3", "利润"]}',
        "那利润呢？",
    )
    assert result.rewritten_query == "2024年Q3的利润是多少"
    assert result.need_rewrite is True
    assert result.expanded_keywords == ["2024", "Q3", "利润"]
    assert result.reason == ""


def test_parse_fenced_json_code_block() -> None:
    """qwen3 用 markdown 代码块包裹 JSON → 仍能解析。"""
    result = parse_rewrite_response(
        '```json\n{"rewritten_query": "什么是增量索引", "need_rewrite": false, '
        '"expanded_keywords": ["增量索引"]}\n```',
        "什么是增量索引",
    )
    assert result.rewritten_query == "什么是增量索引"
    assert result.need_rewrite is False


def test_parse_json_with_surrounding_text() -> None:
    """JSON 前后有说明文本 → 按花括号截取解析。"""
    result = parse_rewrite_response(
        '改写结果如下：\n{"rewritten_query": "视频自动分类", "need_rewrite": true, '
        '"expanded_keywords": ["视频"]}\n以上。',
        "视频也行吗？",
    )
    assert result.rewritten_query == "视频自动分类"


def test_parse_malformed_json_falls_back() -> None:
    """畸形 JSON → 返回原查询，need_rewrite=False，reason 标记解析失败。"""
    result = parse_rewrite_response("{not valid json", "原问题")
    assert result.rewritten_query == "原问题"
    assert result.need_rewrite is False
    assert "解析失败" in result.reason


def test_parse_non_object_falls_back() -> None:
    """LLM 返回数组而非对象 → 原查询。"""
    result = parse_rewrite_response("[1, 2, 3]", "原问题")
    assert result.rewritten_query == "原问题"


def test_parse_empty_rewritten_query_falls_back() -> None:
    """rewritten_query 为空串 → 原查询（空改写无检索价值）。"""
    result = parse_rewrite_response(
        '{"rewritten_query": "", "need_rewrite": true, "expanded_keywords": []}',
        "原问题",
    )
    assert result.rewritten_query == "原问题"
    assert result.need_rewrite is False


# ------------------------------------------------------------------
# rewrite_query 编排（mock call_ollama_json）
# ------------------------------------------------------------------


async def test_rewrite_no_history_skips_llm() -> None:
    """无历史 → 不调 LLM，原样返回。"""

    async def fail_if_called(system: str, user: str) -> str:
        raise AssertionError("无历史不应调用 LLM")

    with mock.patch("app.services.rewrite_service.call_ollama_json", fail_if_called):
        result = await rewrite_query("什么是增量索引", [])

    assert isinstance(result, RewriteResult)
    assert result.rewritten_query == "什么是增量索引"
    assert result.need_rewrite is False
    assert result.expanded_keywords == []


async def test_rewrite_success() -> None:
    """正常 LLM 返回 → 解析为 RewriteResult。"""

    async def fake_call(system: str, user: str) -> str:
        return (
            '{"rewritten_query": "2024年Q3的利润是多少", "need_rewrite": true, '
            '"expanded_keywords": ["2024", "Q3", "利润"]}'
        )

    history = [turn("2024年Q3的营收是多少？", "根据财务报告，2024年Q3营收为 5.2 亿元...")]
    with mock.patch("app.services.rewrite_service.call_ollama_json", fake_call):
        result = await rewrite_query("那利润呢？", history)

    assert result.rewritten_query == "2024年Q3的利润是多少"
    assert result.need_rewrite is True
    assert result.expanded_keywords == ["2024", "Q3", "利润"]


async def test_rewrite_timeout_falls_back() -> None:
    """LLM 超时 → 原查询，reason 标记超时。"""

    async def fake_call(system: str, user: str) -> str:
        raise TimeoutError

    with mock.patch("app.services.rewrite_service.call_ollama_json", fake_call):
        result = await rewrite_query("那利润呢？", [turn()])

    assert result.rewritten_query == "那利润呢？"
    assert result.need_rewrite is False
    assert "超时" in result.reason


async def test_rewrite_llm_unavailable_propagates() -> None:
    """Ollama 不可用 → LLMUnavailableError 冒泡给调用方（T5.6 决定降级）。"""

    async def fake_call(system: str, user: str) -> str:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch("app.services.rewrite_service.call_ollama_json", fake_call),
        pytest.raises(LLMUnavailableError),
    ):
        await rewrite_query("查询", [turn()])


async def test_rewrite_parse_failure_falls_back() -> None:
    """LLM 返回垃圾 → 原查询，reason 标记解析失败。"""

    async def fake_call(system: str, user: str) -> str:
        return "抱歉，我无法完成改写。"

    with mock.patch("app.services.rewrite_service.call_ollama_json", fake_call):
        result = await rewrite_query("原问题", [turn()])

    assert result.rewritten_query == "原问题"
    assert result.need_rewrite is False
    assert "解析失败" in result.reason


# ------------------------------------------------------------------
# T8.5 云端变体（Prompt 版本适配）
# ------------------------------------------------------------------

#: P-02 JSON schema 字段（local/cloud 必须完全一致，DoD 依据）
_REWRITE_SCHEMA_FIELDS = ("rewritten_query", "need_rewrite", "expanded_keywords")


def test_build_prompt_cloud_schema_consistent_with_local() -> None:
    """云端变体 JSON schema 字段与本地完全一致；few-shot 3 → 1。"""
    local_system, local_user = build_rewrite_prompt("那利润呢？", [turn()])
    cloud_system, cloud_user = build_rewrite_prompt("那利润呢？", [turn()], version="cloud")
    for field in _REWRITE_SCHEMA_FIELDS:
        assert field in local_system, field
        assert field in cloud_system, field
    assert cloud_user.count("[示例") == 1
    assert local_user.count("[示例") == 3


async def test_rewrite_query_cloud_provider_uses_generate() -> None:
    """传入云端 Provider → 走 provider.generate(json_mode=True) 并解析。"""
    from app.services.cloud_provider import CloudUnavailableError

    history = [turn(user="2024年Q3营收是多少？", assistant="5.2亿元")]

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
            return (
                '{"rewritten_query": "2024年Q3的利润是多少", "need_rewrite": true,'
                ' "expanded_keywords": ["2024", "利润"]}'
            )

    result = await rewrite_query("那利润呢？", history, provider=FakeCloud())  # type: ignore[arg-type]
    assert result.rewritten_query == "2024年Q3的利润是多少"
    assert len(FakeCloud.calls) == 1
    assert FakeCloud.calls[0]["json_mode"] is True
    assert FakeCloud.calls[0]["temperature"] == 0.0
    assert FakeCloud.calls[0]["max_tokens"] == 256

    class BoomCloud:
        version = "cloud"

        async def generate(self, *args: object, **kwargs: object) -> str:
            raise CloudUnavailableError("proxy down")

    with pytest.raises(LLMUnavailableError):
        await rewrite_query("查询", [turn()], provider=BoomCloud())  # type: ignore[arg-type]
