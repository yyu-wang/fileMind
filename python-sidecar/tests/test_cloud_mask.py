"""云端模式 LLM payload 数据脱敏（安全 07-I-01 / T7.2）单元测试。

覆盖：
1. ``mask_filename``：编号补零、扩展名保留/小写化、无扩展名/非法扩展名省略。
2. ``mask_path_to_depth``：POSIX / Windows 段数、根/空路径。
3. ``truncate_content``：默认/自定义上限、astral 字符不被切断。
4. ``CloudMasker``：同 key 幂等、异 key 编号递增、``resolve`` 还原（DoD）、``stats``。
5. 门控：``is_cloud_masking_active`` / ``content_max`` 默认与 env 覆盖。
6. P-01 出口：激活后 prompt 不含真实文件名/完整路径，含 ``file_001`` 与 ``depth``。
7. P-03 出口：激活后上下文不含真实文件名，来源替换为编号。
8. 批处理共享 masker：多文件编号连续且可还原。
9. ``log_cloud_call``：审计记录 Provider 与数据量，不含内容/文件名。
"""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

from app.core import cloud_mask as cloud_mask_mod
from app.core.cloud_mask import (
    CloudMasker,
    content_max,
    is_cloud_masking_active,
    log_cloud_call,
    mask_filename,
    mask_path_to_depth,
    truncate_content,
)
from app.models import ClassifyItem
from app.rules.llm_classify import ClassifyResult, build_classify_prompt
from app.services import classify_service
from app.services.classify_service import _run_llm_pass
from app.services.generation_service import (
    SourceChunk,
    build_rag_prompt,
    format_context_blocks,
)

if TYPE_CHECKING:
    import pytest


# —— 纯函数 ——


def test_mask_filename_numbers_and_keeps_extension() -> None:
    assert mask_filename("report.pdf", 1) == "file_001.pdf"
    assert mask_filename("report.PDF", 2) == "file_002.pdf"
    assert mask_filename("archive.tar.gz", 3) == "file_003.gz"
    assert mask_filename("noext", 4) == "file_004"


def test_mask_filename_filters_unsafe_extension() -> None:
    # 扩展名含非字母数字 → 省略（防路径注入）
    assert mask_filename("weird.中", 1) == "file_001"
    assert mask_filename("weird.", 2) == "file_002"


def test_mask_path_to_depth_counts_segments() -> None:
    assert mask_path_to_depth("/a/b/c.txt") == 3
    assert mask_path_to_depth(r"C:\a\b\c.txt") == 4  # 含盘符段
    assert mask_path_to_depth("/") == 0
    assert mask_path_to_depth("") == 0


def test_truncate_content_default_and_custom() -> None:
    text = "x" * 1000
    assert len(truncate_content(text)) == 500
    assert len(truncate_content(text, 200)) == 200
    assert truncate_content("short", 500) == "short"


def test_truncate_content_keeps_astral_chars_whole() -> None:
    text = "😀" * 600
    assert truncate_content(text, 500) == "😀" * 500


# —— CloudMasker 注册表 ——


def test_masker_is_idempotent_per_key() -> None:
    masker = CloudMasker()
    first = masker.mask_file("/a/one.pdf", "one.pdf", "/a/one.pdf", "content")
    second = masker.mask_file("/a/one.pdf", "one.pdf", "/a/one.pdf", "content")
    assert first.seq == second.seq == 1
    assert first.mask_name == second.mask_name == "file_001.pdf"
    assert masker.stats()["files"] == 1


def test_masker_increments_seq_across_files() -> None:
    masker = CloudMasker()
    m1 = masker.mask_file("/a/one.pdf", "one.pdf", "/a/one.pdf", "c")
    m2 = masker.mask_file("/b/two.txt", "two.txt", "/b/two.txt", "c")
    assert m1.mask_name == "file_001.pdf"
    assert m2.mask_name == "file_002.txt"


def test_masker_resolve_roundtrip() -> None:
    """DoD：编号可还原到真实文件名/路径。"""
    masker = CloudMasker()
    masker.mask_file("/a/secret.pdf", "secret.pdf", "/a/secret.pdf", "c")
    resolved = masker.resolve("/a/secret.pdf")
    assert resolved is not None
    assert resolved.real_name == "secret.pdf"
    assert resolved.real_path == "/a/secret.pdf"
    assert masker.resolve("/a/nope.pdf") is None


def test_masker_depth_and_stats() -> None:
    masker = CloudMasker(content_limit=10)
    masker.mask_file("/x/y/z.txt", "z.txt", "/x/y/z.txt", "0123456789ABCDEF")
    masked = masker.resolve("/x/y/z.txt")
    assert masked is not None
    assert masked.depth == 3
    assert masked.content_head == "0123456789"
    assert masker.stats() == {"files": 1, "chars": 10}


# —— 门控 ——


def test_masking_gate_default_off() -> None:
    assert is_cloud_masking_active() is False


def test_masking_gate_env_on(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("FILEMIND_CLOUD_MASKING", "1")
    assert is_cloud_masking_active() is True


def test_content_max_default_and_env(monkeypatch: pytest.MonkeyPatch) -> None:
    assert content_max() == 500
    monkeypatch.setenv("FILEMIND_CLOUD_CONTENT_MAX", "2000")
    assert content_max() == 2000
    monkeypatch.setenv("FILEMIND_CLOUD_CONTENT_MAX", "abc")
    assert content_max() == 500
    monkeypatch.setenv("FILEMIND_CLOUD_CONTENT_MAX", "0")
    assert content_max() == 500


# —— P-01 分类出口 ——


def _sample_item() -> ClassifyItem:
    return ClassifyItem(
        name="secret_report.pdf",
        path="/Users/wangyu/Desktop/secret_report.pdf",
        size=1024,
        content_summary="A" * 600 + "MARKER" + "B" * 600,
        modified_time="2026-08-23 10:00",
    )


def test_p01_prompt_masked_with_explicit_masker() -> None:
    item = _sample_item()
    masker = CloudMasker()
    system, user = build_classify_prompt(item, ["文档"], masker)

    assert "secret_report" not in user
    assert "/Users/wangyu" not in user
    assert "file_001.pdf" in user
    assert "所在目录：depth=4" in user
    # 内容截断：500 字符之后的内容不进 payload
    assert "MARKER" not in user
    assert len(system) > 0
    # DoD 还原
    assert masker.resolve(item.path) is not None
    assert masker.resolve(item.path).real_name == "secret_report.pdf"


def test_p01_prompt_masked_when_gate_on(monkeypatch: pytest.MonkeyPatch) -> None:
    """门控激活时无需调用方传 masker，出口自动脱敏。"""
    monkeypatch.setenv("FILEMIND_CLOUD_MASKING", "1")
    item = _sample_item()
    _, user = build_classify_prompt(item, ["文档"])
    assert "secret_report" not in user
    assert "file_001.pdf" in user


def test_p01_prompt_unmasked_when_gate_off() -> None:
    item = _sample_item()
    _, user = build_classify_prompt(item, ["文档"])
    assert "文件名：secret_report.pdf" in user
    assert "/Users/wangyu/Desktop/secret_report.pdf" in user


# —— P-03 聊天出口 ——


def _chunks() -> list[SourceChunk]:
    return [
        SourceChunk(citation_id=1, file_name="secret_notes.txt", page=1, text="机密笔记" * 200),
        SourceChunk(citation_id=2, file_name="secret_notes.txt", page=2, text="更多内容" * 200),
        SourceChunk(citation_id=3, file_name="budget.xlsx.md", page=1, text="预算表" * 200),
    ]


def test_p03_prompt_masked_when_gate_on(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("FILEMIND_CLOUD_MASKING", "1")
    _, user = build_rag_prompt("预算多少", _chunks())
    assert "secret_notes" not in user
    assert "budget" not in user
    # 同文件多片段共享同一编号，不同文件编号递增
    assert "来源：file_001.txt" in user
    assert "来源：file_002.md" in user


def test_p03_context_masked_when_gate_on(monkeypatch: pytest.MonkeyPatch) -> None:
    """P-04 自纠正上下文（format_context_blocks 直接调用）同样脱敏。"""
    monkeypatch.setenv("FILEMIND_CLOUD_MASKING", "1")
    context = format_context_blocks(_chunks())
    assert "secret_notes" not in context
    assert "来源：file_001.txt" in context
    assert "来源：file_002.md" in context


def test_p03_unmasked_when_gate_off() -> None:
    _, user = build_rag_prompt("预算多少", _chunks())
    assert "来源：secret_notes.txt" in user
    assert "file_001" not in user


# —— 批处理共享 masker ——


def test_llm_pass_shares_masker_across_batch(monkeypatch: pytest.MonkeyPatch) -> None:
    """批内共享 masker：多文件编号连续（file_001/file_002）且可还原。"""
    monkeypatch.setenv("FILEMIND_CLOUD_MASKING", "1")
    seen: list[CloudMasker | None] = []

    async def recording_classify(item, categories, masker=None):
        seen.append(masker)
        # 模拟真实链路：出口会调用 build_classify_prompt 完成脱敏登记
        build_classify_prompt(item, categories, masker)
        return ClassifyResult(category="文档", confidence=0.9, reason="t", is_new_category=False)

    monkeypatch.setattr(classify_service, "classify_file_with_llm", recording_classify)

    items = [
        ClassifyItem(name="a.pdf", path="/x/a.pdf", content_summary=""),
        ClassifyItem(name="b.txt", path="/y/b.txt", content_summary=""),
    ]
    results: list[dict[str, object]] = [{}, {}]
    asyncio.run(_run_llm_pass(items, [0, 1], ["文档"], results))

    assert len(seen) == 2
    assert seen[0] is not None and seen[1] is not None
    assert seen[0] is seen[1]  # 同一共享实例
    assert seen[0].resolve("/x/a.pdf").mask_name == "file_001.pdf"
    assert seen[0].resolve("/y/b.txt").mask_name == "file_002.txt"


# —— 审计记录 ——


class _FakeLogger:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, object]]] = []

    def info(self, msg: str, **kwargs: object) -> None:
        self.calls.append((msg, kwargs))


def test_log_cloud_call_records_provider_and_volume(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    masker = CloudMasker()
    masker.mask_file("/a/secret.pdf", "secret.pdf", "/a/secret.pdf", "机密内容")
    fake = _FakeLogger()
    monkeypatch.setattr(cloud_mask_mod, "logger", fake)

    log_cloud_call("openai", masker)

    assert len(fake.calls) == 1
    msg, kwargs = fake.calls[0]
    assert msg == "cloud.call"
    assert kwargs["provider"] == "openai"
    assert kwargs["files"] == 1
    assert kwargs["chars"] == 4
    # 审计不含内容与文件名
    assert "secret.pdf" not in repr(kwargs)
    assert "机密内容" not in repr(kwargs)
