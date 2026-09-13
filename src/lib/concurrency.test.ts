// mapWithConcurrency 单元测试：结果同序、并发上限、异常传播、空输入。

import { describe, expect, it, vi } from 'vitest';
import { mapWithConcurrency } from './concurrency';

/** 指定延迟后返回值的异步任务。 */
function delayed<T>(value: T, ms: number): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(value), ms));
}

describe('mapWithConcurrency', () => {
  it('returns results in input order regardless of completion order', async () => {
    const results = await mapWithConcurrency([30, 10, 20], 3, (ms) => delayed(ms, ms));
    expect(results).toEqual([30, 10, 20]);
  });

  it('never exceeds the concurrency limit', async () => {
    let active = 0;
    let peak = 0;
    await mapWithConcurrency(
      Array.from({ length: 12 }, (_, i) => i),
      3,
      async () => {
        active += 1;
        peak = Math.max(peak, active);
        await delayed(null, 5);
        active -= 1;
      },
    );
    expect(peak).toBe(3);
  });

  it('processes all items exactly once', async () => {
    const seen: number[] = [];
    const items = [1, 2, 3, 4, 5, 6, 7];
    await mapWithConcurrency(items, 2, async (n) => {
      seen.push(n);
      return n * 2;
    });
    expect([...seen].sort((a, b) => a - b)).toEqual(items);
  });

  it('propagates worker errors and does not leave dangling lanes', async () => {
    await expect(
      mapWithConcurrency([1, 2, 3], 2, async (n) => {
        if (n === 2) throw new Error('boom');
        return n;
      }),
    ).rejects.toThrow('boom');
  });

  it('handles empty input', async () => {
    const results = await mapWithConcurrency([], 5, async (n: number) => n);
    expect(results).toEqual([]);
  });

  it('clamps concurrency above item count', async () => {
    const spy = vi.fn(async (n: number) => n);
    const results = await mapWithConcurrency([1, 2], 10, spy);
    expect(results).toEqual([1, 2]);
    expect(spy).toHaveBeenCalledTimes(2);
  });
});
