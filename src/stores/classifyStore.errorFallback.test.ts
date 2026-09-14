// classifyStore 单元测试：IPC 异常兜底——异常后状态不得卡死，且各自遵守既有契约。

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
import { makeItem, makePreview, okExecute, resetClassifyTestState } from './classifyStoreFixtures';

beforeEach(() => {
  resetClassifyTestState();
});

describe('IPC 异常兜底：状态不得卡死', () => {
  it('generatePreview 异常：复位 Idle 并置 error，后续预览不再被重入守卫吞掉', async () => {
    vi.mocked(fileIpc.classifyPreview).mockRejectedValueOnce(new Error('IPC channel closed'));

    // 调用方是 `void generatePreview(ids)`：不 reject，错误走 store.error 横幅
    await expect(useClassifyStore.getState().generatePreview(['a'])).resolves.toBeUndefined();

    let state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.error).toBe('IPC channel closed');

    // 关键回归点：status 若停在 Previewing，这次调用会被重入守卫直接 return——
    // 预览再也生成不出来（页面永久停在「生成中」）
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview([makeItem('a')]),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    state = useClassifyStore.getState();
    expect(state.preview?.items).toHaveLength(1);
  });

  it('execute 异常：按取消语义收尾——不卡 Running，保留部分结果与整批撤销入口', async () => {
    const ids = Array.from({ length: 60 }, (_, i) => `f${i}`);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(ids.map((id) => makeItem(id))),
    });
    await useClassifyStore.getState().generatePreview(ids);
    // 第 1 块成功（50 项），第 2 块 invoke 抛出
    vi.mocked(fileIpc.executeOperations)
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) })
      .mockRejectedValueOnce(new Error('IPC channel closed'));

    await expect(useClassifyStore.getState().execute()).resolves.toBeUndefined();

    const state = useClassifyStore.getState();
    // 卡在 Running 会让「开始分类」永久禁用，且重入守卫拒绝再次执行
    expect(state.status).toBe(ClassifyStatus.Cancelled);
    expect(state.error).toBe('IPC channel closed');
    // 呈现真实的部分结果，而不是 0/0
    expect(state.execSummary).toMatchObject({ success: 50, failed: 0, total: 60 });
    expect(state.lastBatchId).toBe('b1');
  });

  it('execute 异常但已被 reset 作废：不覆盖 reset 后的状态（令牌守卫优先）', async () => {
    const ids = Array.from({ length: 60 }, (_, i) => `f${i}`);
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(ids.map((id) => makeItem(id))),
    });
    await useClassifyStore.getState().generatePreview(ids);
    let rejectSecond!: (err: unknown) => void;
    vi.mocked(fileIpc.executeOperations)
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) })
      .mockImplementationOnce(
        () =>
          new Promise<never>((_resolve, reject) => {
            rejectSecond = reject;
          }),
      );

    const running = useClassifyStore.getState().execute();
    for (let i = 0; i < 20; i += 1) {
      if (vi.mocked(fileIpc.executeOperations).mock.calls.length >= 2) break;
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(2);

    useClassifyStore.getState().reset();
    rejectSecond(new Error('IPC channel closed'));
    await running;

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.error).toBeNull();
  });

  it('loadCategories / refreshCategories / undoLastBatch 异常：均不 reject，且各自遵守既有契约', async () => {
    // loadCategories：写 error 横幅
    vi.mocked(fileIpc.listCategories).mockRejectedValueOnce(new Error('IPC channel closed'));
    await expect(useClassifyStore.getState().loadCategories()).resolves.toBeUndefined();
    expect(useClassifyStore.getState().error).toBe('IPC channel closed');

    // refreshCategories：静默（规则页有自己的失败提示，此处不污染分类页错误）
    useClassifyStore.setState({ error: null });
    vi.mocked(fileIpc.listCategories).mockRejectedValueOnce(new Error('IPC channel closed'));
    await expect(useClassifyStore.getState().refreshCategories()).resolves.toBeUndefined();
    expect(useClassifyStore.getState().error).toBeNull();

    // undoLastBatch：写 error 横幅（调用方是 `void undoLastBatch()`，界面需有提示）
    useClassifyStore.setState({ lastBatchId: 'b1' });
    vi.mocked(fileIpc.undoBatch).mockRejectedValueOnce(new Error('IPC channel closed'));
    await expect(useClassifyStore.getState().undoLastBatch()).resolves.toBeUndefined();
    expect(useClassifyStore.getState().error).toBe('IPC channel closed');
  });
});
