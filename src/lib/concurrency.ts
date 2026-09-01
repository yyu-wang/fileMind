// 有限并发执行异步任务的工具（分类打标等批量 IPC 用）。
//
// 与 Promise.all 的区别：限制同时在途的 promise 数量，避免瞬时压垮
// IPC/数据库；与串行 for-await 的区别：多条通道并行，缩短总等待。

/** 有限并发执行异步任务，结果与输入同序。并发数自动收敛到 [1, items.length]。 */
export async function mapWithConcurrency<T, R>(
  items: readonly T[],
  concurrency: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(items.length);
  if (items.length === 0) return results;

  let next = 0;
  const laneCount = Math.max(1, Math.min(concurrency, items.length));

  async function lane(): Promise<void> {
    while (next < items.length) {
      const index = next;
      next += 1;
      results[index] = await worker(items[index], index);
    }
  }

  await Promise.all(Array.from({ length: laneCount }, () => lane()));
  return results;
}
