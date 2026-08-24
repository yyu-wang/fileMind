"""T10.2 — services.query_cache 单元测试（LRU + TTL + 并发，注入假时钟无 sleep）。

覆盖：miss→set→hit 与命中统计、TTL 过期清理、容量满时 LRU 淘汰、并发读写
不损坏、clear 重置。pyproject 配 asyncio_mode=auto，async 测试自动运行。
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # noqa: E402

from app.services.query_cache import QueryCache  # noqa: E402


class _FakeClock:
    """可推进的单调时钟，避免测试依赖真实 sleep。"""

    def __init__(self, start: float = 0.0) -> None:
        self._now = start

    def __call__(self) -> float:
        return self._now

    def advance(self, seconds: float) -> None:
        self._now += seconds


async def test_miss_then_set_hit() -> None:
    """未命中返回 None；写入后再取返回原值并计入命中统计。"""
    clock = _FakeClock()
    cache: QueryCache[str] = QueryCache(capacity=8, ttl_seconds=60, now=clock)
    key = ("q", "history")
    assert await cache.get(key) is None
    await cache.set(key, "value")
    assert await cache.get(key) == "value"
    assert cache.hits == 1
    assert cache.misses == 1


async def test_ttl_expiry_clears_entry() -> None:
    """超过 TTL 的条目读取时被清掉，返回 None 且 len 归零。"""
    clock = _FakeClock()
    cache: QueryCache[str] = QueryCache(capacity=8, ttl_seconds=60, now=clock)
    key = ("q",)
    await cache.set(key, "v")
    clock.advance(60.1)
    assert await cache.get(key) is None
    assert len(cache) == 0
    # 恰好未过 TTL 仍命中
    await cache.set(key, "v2")
    clock.advance(59.9)
    assert await cache.get(key) == "v2"


async def test_lru_eviction_drops_oldest() -> None:
    """容量满时淘汰最久未用（队首）；最近访问的保留。"""
    cache: QueryCache[str] = QueryCache(capacity=2, ttl_seconds=60)
    await cache.set(("a",), "1")
    await cache.set(("b",), "2")
    # 访问 a → a 成为最近使用
    assert await cache.get(("a",)) == "1"
    await cache.set(("c",), "3")  # 容量 2，应淘汰 b（最久未用）
    assert await cache.get(("b",)) is None
    assert await cache.get(("c",)) == "3"
    assert await cache.get(("a",)) == "1"


async def test_concurrent_get_set_no_corruption() -> None:
    """并发读写不损坏缓存（写后读一致，无丢条目）。"""
    cache: QueryCache[int] = QueryCache(capacity=64, ttl_seconds=60)
    keys = [(f"k{i}",) for i in range(50)]

    async def worker(i: int) -> None:
        key = keys[i]
        await cache.set(key, i)
        assert await cache.get(key) == i

    await asyncio.gather(*(worker(i) for i in range(50)))
    assert len(cache) == 50


async def test_set_over_capacity_drops_oldest_entries() -> None:
    """批量写入超过容量 → 只保留最近 capacity 条。"""
    cache: QueryCache[int] = QueryCache(capacity=3, ttl_seconds=60)
    for i in range(10):
        await cache.set((f"k{i}",), i)
    assert len(cache) == 3
    assert await cache.get(("k9",)) == 9
    assert await cache.get(("k7",)) == 7
    assert await cache.get(("k0",)) is None  # 最早的已被淘汰


async def test_clear_resets_entries_and_stats() -> None:
    """clear() 清空条目并重置命中统计。"""
    cache: QueryCache[str] = QueryCache(capacity=4, ttl_seconds=60)
    await cache.set(("a",), "1")
    assert await cache.get(("a",)) == "1"
    await cache.clear()
    assert len(cache) == 0
    assert cache.hits == 0
    assert cache.misses == 0
