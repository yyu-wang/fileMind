// classifyStore 单元测试：预览生成（含 FE-M9 重入/过期响应守卫）与分类缓存加载。

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    classifyPreview: vi.fn(),
    executeOperations: vi.fn(),
    updateFileCategory: vi.fn(),
    undoBatch: vi.fn(),
    listAllFiles: vi.fn(),
    getFileStats: vi.fn(),
    scanDirectory: vi.fn(),
    listCategories: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import { ClassifyStatus } from '../types/models';
import { useClassifyStore } from './classifyStore';
import { useFileStore } from './fileStore';
import { makeItem, makePreview, resetClassifyTestState, SCAN_ROOT } from './classifyStoreFixtures';

beforeEach(() => {
  resetClassifyTestState();
});

describe('generatePreview（FE-M9 守卫）', () => {
  it('FE-M9: Previewing 中重入直接返回（classifyPreview 仅一次）', async () => {
    // 手写 deferred：类型与 mock 返回联合对齐，避免 unknown 不兼容
    let resolvePreview!: (v: Awaited<ReturnType<typeof fileIpc.classifyPreview>>) => void;
    vi.mocked(fileIpc.classifyPreview).mockImplementation(
      () =>
        new Promise((r) => {
          resolvePreview = r as typeof resolvePreview;
        }),
    );

    const p1 = useClassifyStore.getState().generatePreview(['a']);
    const p2 = useClassifyStore.getState().generatePreview(['a']);
    resolvePreview({ status: 'ok', data: makePreview([makeItem('a')]) });
    await Promise.all([p1, p2]);

    expect(fileIpc.classifyPreview).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
  });

  it('FE-M9: 过期响应丢弃——reset 后到达的慢响应不覆盖状态', async () => {
    let resolvePreview!: (v: Awaited<ReturnType<typeof fileIpc.classifyPreview>>) => void;
    vi.mocked(fileIpc.classifyPreview).mockImplementation(
      () =>
        new Promise((r) => {
          resolvePreview = r as typeof resolvePreview;
        }),
    );

    const p = useClassifyStore.getState().generatePreview(['a']);
    // 用户在响应到达前取消（reset 作废 seq），随后慢响应才到达
    useClassifyStore.getState().reset();
    resolvePreview({ status: 'ok', data: makePreview([makeItem('a')]) });
    await p;

    // reset 后状态保持 Idle/preview=null，不被慢响应覆盖
    const s = useClassifyStore.getState();
    expect(s.status).toBe(ClassifyStatus.Idle);
    expect(s.preview).toBeNull();
  });
});

describe('generatePreview', () => {
  it('calls classifyPreview with scanPath from fileStore and stores pending ids', async () => {
    const preview = makePreview([
      makeItem('a'),
      makeItem('p', { category_name: null, rule_source: 'pending' }),
    ]);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'ok', data: preview });

    await useClassifyStore.getState().generatePreview(['a', 'p']);

    expect(fileIpc.classifyPreview).toHaveBeenCalledWith(['a', 'p'], SCAN_ROOT);
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().preview).toEqual(preview);
    expect(useClassifyStore.getState().pendingIds).toEqual(['p']);
  });

  it('sets error without calling IPC when scanPath is missing', async () => {
    useFileStore.setState({ scanPath: null });
    await useClassifyStore.getState().generatePreview(['a']);

    expect(fileIpc.classifyPreview).not.toHaveBeenCalled();
    expect(useClassifyStore.getState().error).toBe('请先在文件页选择要整理的目录');
  });

  it('sets error and stays Idle when preview fails', async () => {
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({ status: 'error', error: '扫描根无效' });
    await useClassifyStore.getState().generatePreview(['a']);

    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().error).toBe('扫描根无效');
  });
});

describe('loadCategories', () => {
  it('loads categories once and caches (idempotent)', async () => {
    await useClassifyStore.getState().loadCategories();
    expect(fileIpc.listCategories).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().categories).toHaveLength(2);

    await useClassifyStore.getState().loadCategories();
    expect(fileIpc.listCategories).toHaveBeenCalledTimes(1);
  });
});
