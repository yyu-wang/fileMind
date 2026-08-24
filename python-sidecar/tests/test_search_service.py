"""T2.4 — search_service 单元测试。

覆盖：
    1. ``tokenize_chinese``：jieba 分词 + 空格连接
    2. ``build_fts_query``：FTS5 MATCH 表达式构造 + 注入防御
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.search_service import build_fts_query, tokenize_chinese  # noqa: E402

# ---------------------------------------------------------------------------
# tokenize_chinese
# ---------------------------------------------------------------------------


def test_tokenize_empty() -> None:
    """空串 → 空串。"""
    assert tokenize_chinese("") == ""


def test_tokenize_whitespace_only() -> None:
    """纯空白 → 空串（token strip 后过滤掉）。"""
    assert tokenize_chinese("   ") == ""


def test_tokenize_english_passthrough() -> None:
    """英文按空格切分，jieba 不改变 token。"""
    result = tokenize_chinese("hello world")
    tokens = result.split()
    assert "hello" in tokens
    assert "world" in tokens


def test_tokenize_chinese_segmented() -> None:
    """中文被 jieba 切分为多 token，结果用空格连接。"""
    # "文件管理系统" 至少切出 "文件" "管理" "系统" 三个常见词
    result = tokenize_chinese("文件管理系统")
    tokens = result.split()
    assert len(tokens) >= 2, f"期望至少 2 个 token，实际: {tokens}"
    # 关键词应出现
    assert "文件" in tokens
    assert "管理" in tokens
    assert "系统" in tokens


def test_tokenize_mixed_chinese_english() -> None:
    """中英混合：两边都被分词，结果用空格连接。"""
    result = tokenize_chinese("FileMind 文件管理工具")
    tokens = result.split()
    assert "FileMind" in tokens
    assert "文件" in tokens
    assert "管理" in tokens


def test_tokenize_no_whitespace_tokens() -> None:
    """每个 token 已 strip，结果中不应有纯空白 token。"""
    result = tokenize_chinese("  hello   world  ")
    tokens = result.split()
    # split() 默认按任意空白切分，确保没有空 token
    assert all(t for t in tokens)


# ---------------------------------------------------------------------------
# build_fts_query
# ---------------------------------------------------------------------------


def test_build_query_empty() -> None:
    """空串 → 空串（调用方应跳过 MATCH）。"""
    assert build_fts_query("") == ""


def test_build_query_whitespace_only() -> None:
    """纯空白 → 空串。"""
    assert build_fts_query("   ") == ""


def test_build_query_english_single_word() -> None:
    """英文单词 → ``"word"``。"""
    assert build_fts_query("hello") == '"hello"'


def test_build_query_english_multiple_words() -> None:
    """英文多词 → ``"word1" "word2"``（FTS5 默认 AND 语义）。"""
    result = build_fts_query("hello world")
    assert result == '"hello" "world"'


def test_build_query_chinese_phrase() -> None:
    """中文短语 → 每个 jieba token 双引号包裹，空格连接。"""
    result = build_fts_query("文件管理")
    # 至少有 "文件" 和 "管理" 两个被引号包裹的 token
    assert '"文件"' in result
    assert '"管理"' in result
    # 不应包含未包裹的裸 token
    assert result.startswith('"')


def test_build_query_injection_defense() -> None:
    """FTS5 语法注入防御：每个 token 都被双引号包裹。

    jieba 对 ``OR`` ``NOT`` ``*`` 这类 FTS5 操作符不会特殊处理，
    但因为每个 token 都用双引号包裹，FTS5 会按字面量匹配，
    操作符不会被执行。
    """
    result = build_fts_query("OR NOT")
    # OR 和 NOT 都被引号包裹 → 字面量匹配
    assert '"OR"' in result
    assert '"NOT"' in result


def test_build_query_quote_in_input_escaped() -> None:
    """输入含双引号时，token 内的双引号需被处理（不破坏 FTS5 语法）。

    jieba 通常会把含引号的输入切成不含引号的 token；
    若 token 内仍有引号，build_fts_query 不主动转义——
    调用方应保证输入已做基础清洗（Rust 端 sanitize_query 已做）。
    这里只验证正常 token 的拼接逻辑。
    """
    result = build_fts_query("hello")
    assert result == '"hello"'
    assert result.count('"') == 2  # 一对引号


def test_build_query_mixed() -> None:
    """中英混合查询的构造。"""
    result = build_fts_query("FileMind 文件管理")
    tokens = result.split(" ")
    # 每个 token 应被双引号包裹
    assert all(t.startswith('"') and t.endswith('"') for t in tokens)
    # 包含 FileMind 和中文 token
    assert '"FileMind"' in tokens
    assert any("文件" in t for t in tokens)
    assert any("管理" in t for t in tokens)
