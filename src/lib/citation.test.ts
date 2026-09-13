// citation.ts 单元测试：findFileByName 四级匹配策略 + Rust FTS 兜底 + 3s 超时保护。
//
// 匹配顺序：精确 → 大小写不敏感 → 去扩展名 → 子串包含 → IPC 兜底（带超时）。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileInfo } from '@/types/ipc';
import { findFileByName } from './citation';

const mocks = vi.hoisted(() => ({
  searchByFilename: vi.fn(),
}));

vi.mock('./ipc', () => ({
  fileIpc: { searchByFilename: mocks.searchByFilename },
}));

function file(name: string): FileInfo {
  return {
    id: `id-${name}`,
    path: `/tmp/${name}`,
    file_name: name,
    file_size: 100,
    content_hash: null,
    category: null,
    created_at: '2026-08-01 00:00:00',
    updated_at: '2026-08-01 00:00:00',
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('findFileByName', () => {
  it('1) 精确命中（大小写敏感）不走 IPC', async () => {
    const target = file('report.md');
    const res = await findFileByName('report.md', [file('a.txt'), target]);
    expect(res?.file).toBe(target);
    expect(res?.fastPath).toBe(true);
    expect(mocks.searchByFilename).not.toHaveBeenCalled();
  });

  it('2) 大小写不敏感命中', async () => {
    const target = file('Report.md');
    const res = await findFileByName('report.md', [target]);
    expect(res?.file).toBe(target);
    expect(res?.fastPath).toBe(true);
  });

  it('3) 去扩展名匹配（citation 丢了后缀）', async () => {
    const target = file('notes.md');
    const res = await findFileByName('notes', [target]);
    expect(res?.file).toBe(target);
    expect(mocks.searchByFilename).not.toHaveBeenCalled();
  });

  it('4) 子串双向包含匹配（文件名包含引用名）', async () => {
    const target = file('filemind-设计文档-v2.md');
    const res = await findFileByName('设计文档', [target]);
    expect(res?.file).toBe(target);
  });

  it('4b) 子串双向包含匹配（引用名包含文件名）', async () => {
    const target = file('预算表');
    const res = await findFileByName('2026预算表-final', [target]);
    expect(res?.file).toBe(target);
  });

  it('5) 兜底 IPC：优先取大小写不敏感精确项，fastPath=false', async () => {
    const exact = file('X.md');
    const other = file('X-副本.md');
    mocks.searchByFilename.mockResolvedValue({ status: 'ok', data: [other, exact] });
    const res = await findFileByName('x.md', []);
    expect(res?.file).toBe(exact);
    expect(res?.fastPath).toBe(false);
    expect(mocks.searchByFilename).toHaveBeenCalledWith('x.md', 5);
  });

  it('6) 兜底 IPC 无精确项时取第一个结果', async () => {
    const other = file('X-副本.md');
    mocks.searchByFilename.mockResolvedValue({ status: 'ok', data: [other] });
    const res = await findFileByName('x', []);
    expect(res?.file).toBe(other);
  });

  it('7) 兜底 IPC 空结果返回 null', async () => {
    mocks.searchByFilename.mockResolvedValue({ status: 'ok', data: [] });
    await expect(findFileByName('missing', [])).resolves.toBeNull();
  });

  it('8) 兜底 IPC 业务错误返回 null', async () => {
    mocks.searchByFilename.mockResolvedValue({ status: 'error', error: 'boom' });
    await expect(findFileByName('missing', [])).resolves.toBeNull();
  });

  it('9) IPC 卡死时 3s 超时保护', async () => {
    vi.useFakeTimers();
    try {
      mocks.searchByFilename.mockImplementation(() => new Promise<never>(() => undefined));
      const promise = findFileByName('stuck', []);
      const assertion = expect(promise).rejects.toThrow('搜索引用文件超时（3s）');
      await vi.advanceTimersByTimeAsync(3000);
      await assertion;
    } finally {
      vi.useRealTimers();
    }
  });
});
