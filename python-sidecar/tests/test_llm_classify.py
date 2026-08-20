"""T4.4 — LLM 兜底分类层单元测试。

覆盖：P-01 提示词构建、JSON 解析（含越界置信度/解析失败）、置信度阈值
标记、Ollama 调用与超时/不可用兜底（mock 网络，不走真实请求）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from unittest import mock

import httpx
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.models import ClassifyItem  # noqa: E402
from app.rules.llm_classify import (  # noqa: E402
    CONFIDENCE_THRESHOLD,
    ClassifyResult,
    LLMUnavailableError,
    _human_size,
    build_classify_prompt,
    call_ollama_json,
    classify_file_with_llm,
    parse_classify_response,
)


def make_item(
    name: str = "report.pdf",
    extension: str = "pdf",
    path: str = "/data",
    size: int = 2_400_000,
    content_summary: str = "",
    modified_time: str = "2026-08-15T10:30:00Z",
) -> ClassifyItem:
    """构造待分类文件。"""
    return ClassifyItem(
        name=name,
        extension=extension,
        path=path,
        size=size,
        content_summary=content_summary,
        modified_time=modified_time,
    )


def test_parse_valid_json() -> None:
    """合法 JSON → 字段保留，无标记。"""
    result = parse_classify_response(
        '{"category": "文档", "confidence": 0.9, "reason": "pdf文件", "is_new_category": false}'
    )
    assert result.category == "文档"
    assert result.confidence == 0.9
    assert result.reason == "pdf文件"
    assert result.is_new_category is False


def test_parse_high_confidence_no_marker() -> None:
    """高置信度（≥ 阈值）不加 [需人工确认]（阈值默认 0.7）。"""
    assert CONFIDENCE_THRESHOLD == 0.7
    result = parse_classify_response(
        '{"category": "文档", "confidence": 0.95, "reason": "ok", "is_new_category": false}'
    )
    assert not result.reason.startswith("[需人工确认]")


def test_parse_low_confidence_marker() -> None:
    """低置信度（< 阈值）reason 加 [需人工确认] 前缀。"""
    result = parse_classify_response(
        '{"category": "文档", "confidence": 0.5, "reason": "不确定", "is_new_category": false}'
    )
    assert result.reason == "[需人工确认] 不确定"


def test_parse_confidence_out_of_range_degraded() -> None:
    """置信度越界（>1.0）→ 降级 0.5，并加 [需人工确认]。"""
    result = parse_classify_response(
        '{"category": "文档", "confidence": 5.0, "reason": "异常", "is_new_category": false}'
    )
    assert result.confidence == 0.5
    assert result.reason.startswith("[需人工确认]")


def test_parse_negative_confidence_degraded() -> None:
    """置信度越界（<0.0）→ 降级 0.5。"""
    result = parse_classify_response(
        '{"category": "文档", "confidence": -1.0, "reason": "异常", "is_new_category": false}'
    )
    assert result.confidence == 0.5


def test_parse_malformed_json_returns_unclassified() -> None:
    """JSON 解析失败 → 未分类 / 0.0，不抛异常。"""
    result = parse_classify_response("{not valid json")
    assert result.category == "未分类"
    assert result.confidence == 0.0
    assert result.is_new_category is False
    assert "解析失败" in result.reason


def test_parse_missing_field_returns_unclassified() -> None:
    """字段缺失（Pydantic 校验失败）→ 未分类。"""
    result = parse_classify_response('{"category": "文档"}')
    assert result.category == "未分类"
    assert result.confidence == 0.0


def test_parse_non_object_returns_unclassified() -> None:
    """LLM 返回数组而非对象 → 未分类。"""
    result = parse_classify_response("[1, 2, 3]")
    assert result.category == "未分类"


def test_parse_new_category_flag_passthrough() -> None:
    """is_new_category 原样透传。"""
    result = parse_classify_response(
        '{"category": "自定义类", "confidence": 0.85, "reason": "ok", "is_new_category": true}'
    )
    assert result.category == "自定义类"
    assert result.is_new_category is True


def test_parse_fenced_json_code_block() -> None:
    """qwen3 等模型可能用 markdown 代码块包裹 JSON → 仍能解析。"""
    result = parse_classify_response(
        '```json\n{"category": "文档", "confidence": 0.9, "reason": "pdf", "is_new_category": false}\n```'
    )
    assert result.category == "文档"
    assert result.confidence == 0.9


def test_parse_json_with_surrounding_text() -> None:
    """JSON 前后有说明文本 → 按首个/末个花括号截取解析。"""
    result = parse_classify_response(
        '根据分析，结果为：\n{"category": "代码", "confidence": 0.8, "reason": "py", "is_new_category": false}\n以上。'
    )
    assert result.category == "代码"
    assert result.confidence == 0.8


def test_build_prompt_system_has_categories() -> None:
    """SYSTEM 包含预定义分类列表（顿号分隔）。"""
    system, _ = build_classify_prompt(make_item(), ["财务", "文档"])
    assert "财务、文档" in system
    assert "严格 JSON" in system


def test_build_prompt_user_has_file_info_and_fewshot() -> None:
    """USER 包含文件信息、人类可读大小、Few-shot 与 [待分类]。"""
    item = make_item(name="2024Q3财务报告.xlsx", extension="xlsx", size=2_400_000)
    _, user = build_classify_prompt(item, ["财务"])
    assert "2024Q3财务报告.xlsx" in user
    assert "xlsx" in user
    assert "2.3 MB" in user
    assert "示例 1" in user and "示例 3" in user
    assert "[待分类]" in user


def test_build_prompt_truncates_content_summary() -> None:
    """内容摘要截断到 500 字符。"""
    item = make_item(content_summary="x" * 600)
    _, user = build_classify_prompt(item, [])
    assert "x" * 500 in user
    assert "x" * 501 not in user


def test_build_prompt_guesses_type_from_name() -> None:
    """extension 缺省时从文件名猜测文件类型。"""
    item = make_item(name="notes.bkp", extension="")
    _, user = build_classify_prompt(item, [])
    assert "bkp" in user


def test_human_size() -> None:
    """字节数 → 人类可读大小。"""
    assert _human_size(0) == "0 B"
    assert _human_size(500) == "500 B"
    assert _human_size(2_400) == "2.3 KB"
    assert _human_size(2_400_000) == "2.3 MB"
    assert _human_size(2_400_000_000) == "2.2 GB"


async def testcall_ollama_json_returns_content() -> None:
    """正常响应 → 返回 message.content。"""

    async def fake_chat(**kwargs: object) -> dict[str, object]:
        return {"message": {"content": '{"category":"文档"}'}}

    fake_client = mock.MagicMock()
    fake_client.chat = fake_chat
    with mock.patch("app.rules.llm_classify.AsyncClient", return_value=fake_client):
        assert await call_ollama_json("sys", "user") == '{"category":"文档"}'


async def testcall_ollama_json_conn_error_raises_unavailable() -> None:
    """连接失败（httpx.HTTPError）→ LLMUnavailableError。"""

    async def raise_conn(**kwargs: object) -> dict[str, object]:
        raise httpx.ConnectError("connection refused")

    fake_client = mock.MagicMock()
    fake_client.chat = raise_conn
    with (
        mock.patch("app.rules.llm_classify.AsyncClient", return_value=fake_client),
        pytest.raises(LLMUnavailableError),
    ):
        await call_ollama_json("sys", "user")


async def test_classify_file_with_llm_success() -> None:
    """正常 LLM 返回 → 解析为 ClassifyResult。"""

    async def fake_call(system: str, user: str) -> str:
        return '{"category": "财务", "confidence": 0.9, "reason": "xlsx报表", "is_new_category": false}'

    with mock.patch("app.rules.llm_classify.call_ollama_json", fake_call):
        result = await classify_file_with_llm(make_item(), ["财务"])
    assert isinstance(result, ClassifyResult)
    assert result.category == "财务"
    assert result.confidence == 0.9


async def test_classify_file_with_llm_timeout_returns_unclassified() -> None:
    """超时 → 未分类，reason 标记 LLM 超时。"""

    async def fake_call(system: str, user: str) -> str:
        raise TimeoutError

    with mock.patch("app.rules.llm_classify.call_ollama_json", fake_call):
        result = await classify_file_with_llm(make_item(), [])
    assert result.category == "未分类"
    assert result.confidence == 0.0
    assert "超时" in result.reason


async def test_classify_file_with_llm_unavailable_propagates() -> None:
    """Ollama 不可用 → LLMUnavailableError 冒泡给调用方。"""

    async def fake_call(system: str, user: str) -> str:
        raise LLMUnavailableError("Ollama down")

    with (
        mock.patch("app.rules.llm_classify.call_ollama_json", fake_call),
        pytest.raises(LLMUnavailableError),
    ):
        await classify_file_with_llm(make_item(), [])
