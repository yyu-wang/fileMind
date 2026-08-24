"""T10.2 — RAG 检索结果查询缓存（LRU + TTL，缓存**重排后**结果）。

设计背景（T10.2「内存优先（懒加载）」取舍的配套措施）：
    rerank 模型保持懒加载不预载，重复问句的首 token 延迟主要由检索管线
    （改写 → 向量化 → 重排）贡献。查询缓存命中时整条管线跳过，直接把上次
    重排结果当作本次检索结果，把「模型加载 + 推理」降为一次 dict 查找。

关键点：
    - key 覆盖影响检索结果的输入：query + 对话历史 + 表名 + 模型 + top_k 参数。
      FTS 命中（``fts_chunks``）由同一 query 确定性产生，无需入 key。
    - 缓存的是**重排后**结果（``_retrieve`` 返回值），命中时连 rewritten_query
      一起复用，保证生成上下文与检索一致。
    - TTL 默认 60s：避免索引重建 / 文件变更后长时间读到过期检索。
    - asyncio.Lock 串行化 OrderedDict 访问（单事件循环内足够）。

测试：``tests/test_query_cache.py``（命中 / 过期 / 淘汰 / 并发，注入假时钟）。
"""

from __future__ import annotations

import asyncio
import os
import time
from collections import OrderedDict
from typing import TYPE_CHECKING

from app.services.generation_service import SourceChunk

if TYPE_CHECKING:
    from collections.abc import Callable

#: 缓存条目数上限（env 可覆盖；默认 64 条，内存开销可忽略）
CACHE_CAPACITY = int(os.environ.get("FILEMIND_QUERY_CACHE_SIZE", "64") or "64")
#: 缓存 TTL（秒，env 可覆盖）
CACHE_TTL_SECONDS = float(os.environ.get("FILEMIND_QUERY_CACHE_TTL", "60") or "60")

#: 检索管线缓存值：``(rewritten_query, candidates, sources, chunks)``（``_retrieve`` 返回值）。
_RetrieveValue = tuple[str, int, list[dict[str, object]], list[SourceChunk]]


class QueryCache[T]:
    """asyncio 安全 LRU 缓存（``get``/``set``/``clear`` + 命中统计）。

    ``now`` 可注入（测试用假时钟）；默认 ``time.monotonic``。
    """

    def __init__(
        self,
        capacity: int = CACHE_CAPACITY,
        ttl_seconds: float = CACHE_TTL_SECONDS,
        *,
        now: Callable[[], float] = time.monotonic,
    ) -> None:
        self._capacity = max(1, capacity)
        self._ttl = ttl_seconds
        self._now = now
        self._data: OrderedDict[tuple[object, ...], tuple[float, T]] = OrderedDict()
        self._lock = asyncio.Lock()
        self.hits = 0
        self.misses = 0

    async def get(self, key: tuple[object, ...]) -> T | None:
        """返回未过期条目并记为命中；miss / 过期条目返回 ``None``。"""
        async with self._lock:
            item = self._data.get(key)
            if item is None:
                self.misses += 1
                return None
            ts, value = item
            if self._now() - ts > self._ttl:
                del self._data[key]
                self.misses += 1
                return None
            self.hits += 1
            self._data.move_to_end(key)  # 最近使用 → 队尾（LRU 淘汰队首）
            return value

    async def set(self, key: tuple[object, ...], value: T) -> None:
        """写入条目；超容量时淘汰最久未用（队首）。"""
        async with self._lock:
            self._data[key] = (self._now(), value)
            self._data.move_to_end(key)
            while len(self._data) > self._capacity:
                self._data.popitem(last=False)

    async def clear(self) -> None:
        """清空缓存并重置命中统计（索引重建后调用，避免读到过期检索）。"""
        async with self._lock:
            self._data.clear()
            self.hits = 0
            self.misses = 0

    def __len__(self) -> int:
        return len(self._data)


#: 模块级单例（测试用 :func:`reset_query_cache` 重置，避免跨用例污染）
_cache: QueryCache[_RetrieveValue] | None = None


def get_query_cache() -> QueryCache[_RetrieveValue]:
    """返回检索缓存单例。"""
    global _cache
    if _cache is None:
        _cache = QueryCache[_RetrieveValue]()
    return _cache


def reset_query_cache() -> None:
    """重置检索缓存单例（仅测试用）。"""
    global _cache
    _cache = None
