"""FTS5 中文分词与查询构造服务。

SQLite FTS5 的 ``unicode61`` tokenizer 按空格切分 token，不识别中文词边界。
本模块用 jieba 对中文文本预分词，把分词结果用空格连接后写入 FTS content 列；
搜索时同样用 jieba 对查询词分词，构造 FTS5 MATCH 表达式。

安全：每个 token 用双引号包裹防止 FTS5 语法注入（``"token"``），不直接拼用户输入。
"""

from __future__ import annotations

import jieba

# jieba 词典懒加载标记：首次调用时分词方法内部会自动初始化
_jieba_initialized = False


def _ensure_jieba() -> None:
    """首次调用时初始化 jieba 词典（约 40MB 内存，启动时不阻塞）。"""
    global _jieba_initialized
    if not _jieba_initialized:
        jieba.initialize()
        _jieba_initialized = True


def tokenize_chinese(text: str) -> str:
    """用 jieba 对文本分词，返回空格连接的 token 串。

    用于写入 FTS5 content 列：分词后的文本由 ``unicode61`` 按空格切分即可。

    示例：
        ``"文件管理系统"`` → ``"文件 管理 系统"``
        ``"hello world"`` → ``"hello world"``（英文透传）
        ``"FileMind 文件管理工具"`` → ``"FileMind 文件 管理 工具"``
    """
    _ensure_jieba()
    tokens = jieba.cut_for_search(text)
    return " ".join(t.strip() for t in tokens if t.strip())


def build_fts_query(query: str) -> str:
    """构造 FTS5 MATCH 查询表达式。

    步骤：
        1. jieba 分词
        2. 每个 token 用双引号包裹（防 FTS5 语法注入）
        3. 空格连接（FTS5 默认 AND 语义）

    示例：
        ``"文件管理"`` → ``'"文件" "管理"'``
        ``"test"`` → ``'"test"'``
        ``""`` → ``""``

    Returns:
        FTS5 MATCH 表达式字符串；空查询返回空串（调用方应跳过 MATCH）
    """
    _ensure_jieba()
    tokens = jieba.cut_for_search(query)
    safe_tokens = [f'"{t.strip()}"' for t in tokens if t.strip()]
    return " ".join(safe_tokens)
