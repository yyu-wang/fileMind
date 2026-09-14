// classifyStore 单元测试：分块执行（暂停/取消/重入）、整批撤销与状态复位。

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
import type { ExecuteResponse } from '../types/ipc';
import { useClassifyStore } from './classifyStore';
import {
  deferred,
  makeItem,
  makePreview,
  okExecute,
  resetClassifyTestState,
} from './classifyStoreFixtures';

beforeEach(() => {
  resetClassifyTestState();
});

describe('execute', () => {
  it('splits plan into chunks and updates category for each success', async () => {
    const ids = Array.from({ length: 55 }, (_, n) => `f${n}`);
    const items = ids.map((id) => makeItem(id));
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(ids);

    vi.mocked(fileIpc.executeOperations)
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) })
      .mockResolvedValueOnce({ status: 'ok', data: okExecute('b1', ids.slice(50)) });

    await useClassifyStore.getState().execute();

    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(2);
    const firstPlan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    const secondPlan = vi.mocked(fileIpc.executeOperations).mock.calls[1][0].plan;
    expect(firstPlan).toHaveLength(50);
    expect(secondPlan).toHaveLength(5);
    expect(firstPlan[0].operation).toBe('Move');
    expect(firstPlan[0].new_path).toBe(items[0].target_path);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(55);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledWith('f0', '图片');

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.progress).toEqual({ done: 55, total: 55 });
    expect(state.execSummary).toEqual({ success: 55, failed: 0, pending: 0, total: 55 });
    expect(state.lastBatchId).toBe('b1');
  });

  it('copy mode sends Copy operation and still labels original files', async () => {
    const items = [makeItem('a'), makeItem('b')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a', 'b']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a', 'b']),
    });

    await useClassifyStore.getState().execute(false, 'copy');

    const plan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    expect(plan).toHaveLength(2);
    expect(plan[0].operation).toBe('Copy');
    expect(plan[0].new_path).toBe(items[0].target_path);
    // 复制模式同样给原文件打分类标签（软排除，避免重复选中）
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(2);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledWith('a', '图片');

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.execSummary).toEqual({ success: 2, failed: 0, pending: 0, total: 2 });
  });

  it('excludes pending and conflict items from execution plan', async () => {
    const items = [
      makeItem('ok'),
      makeItem('pend', { category_name: null, rule_source: 'pending' }),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'pend', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok']),
    });
    await useClassifyStore.getState().execute();

    const plan = vi.mocked(fileIpc.executeOperations).mock.calls[0][0].plan;
    expect(plan.map((p) => p.file_id)).toEqual(['ok']);
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(1);
    expect(useClassifyStore.getState().execSummary?.pending).toBe(1);
  });

  it('pause and resume toggle status during an in-flight chunk', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    useClassifyStore.getState().pause();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Paused);

    useClassifyStore.getState().resume();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Done);
  });

  it('cancel stops further chunks but keeps executed results', async () => {
    const ids = Array.from({ length: 55 }, (_, n) => `f${n}`);
    const items = ids.map((id) => makeItem(id));
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(ids);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());

    useClassifyStore.getState().cancel();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Cancelled);

    gate.resolve({ status: 'ok', data: okExecute('b1', ids.slice(0, 50)) });
    await execPromise;

    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);
    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Cancelled);
    expect(state.execSummary?.success).toBe(50);
    expect(state.progress).toEqual({ done: 50, total: 55 });
  });

  it('resolveConflicts=true includes conflict items and passes the flag', async () => {
    const items = [
      makeItem('ok'),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok', 'conf']),
    });
    await useClassifyStore.getState().execute(true);

    const req = vi.mocked(fileIpc.executeOperations).mock.calls[0][0];
    expect(req.resolve_conflicts).toBe(true);
    expect(req.plan.map((p) => p.file_id)).toEqual(['ok', 'conf']);
    expect(useClassifyStore.getState().execSummary?.success).toBe(2);
  });

  it('default execute excludes conflict items without resolving', async () => {
    const items = [
      makeItem('ok'),
      makeItem('conf', { status: 'Conflict', conflict_type: 'SameName' }),
    ];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['ok', 'conf']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['ok']),
    });
    await useClassifyStore.getState().execute();

    const req = vi.mocked(fileIpc.executeOperations).mock.calls[0][0];
    expect(req.resolve_conflicts).toBe(false);
    expect(req.plan.map((p) => p.file_id)).toEqual(['ok']);
  });

  // FE-C1：执行中/暂停中再次 execute 直接返回，不产生第二次 IPC。
  it('rejects reentrant execute while Running without a second IPC call', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Running);

    await useClassifyStore.getState().execute();
    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
  });

  it('rejects reentrant execute while Paused without a second IPC call', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());
    useClassifyStore.getState().pause();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Paused);

    await useClassifyStore.getState().execute();
    expect(fileIpc.executeOperations).toHaveBeenCalledTimes(1);

    useClassifyStore.getState().resume();
    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;
  });

  // FE-C5：执行中 reset 后，旧循环收尾不得把状态覆盖回 Cancelled/Done。
  it('reset during execution prevents stale loop from overwriting state', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    const gate = deferred<{ status: 'ok'; data: ExecuteResponse }>();
    vi.mocked(fileIpc.executeOperations).mockReturnValueOnce(gate.promise);

    const execPromise = useClassifyStore.getState().execute();
    await vi.waitFor(() => expect(fileIpc.executeOperations).toHaveBeenCalled());

    useClassifyStore.getState().reset();
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().preview).toBeNull();

    gate.resolve({ status: 'ok', data: okExecute('b1', ['a']) });
    await execPromise;

    // 旧循环恢复后令牌失配：不得写回状态/execSummary/lastBatchId。
    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.execSummary).toBeNull();
    expect(state.lastBatchId).toBeNull();
    expect(state.progress).toEqual({ done: 0, total: 0 });
  });

  // FE-C6：打标失败计入 failed 并提示，后续文件继续执行。
  it('counts label failure into failed and continues remaining items', async () => {
    const items = [makeItem('a'), makeItem('b')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a', 'b']);

    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a', 'b']),
    });
    vi.mocked(fileIpc.updateFileCategory)
      .mockResolvedValueOnce({ status: 'error', error: '数据库锁定' })
      .mockResolvedValueOnce({ status: 'ok', data: null });

    await useClassifyStore.getState().execute();

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Done);
    expect(state.execSummary).toEqual({ success: 1, failed: 1, pending: 0, total: 2 });
    expect(state.error).toContain('分类标签写入失败');
    // 失败不中断整体执行：两个文件的打标调用都发生。
    expect(fileIpc.updateFileCategory).toHaveBeenCalledTimes(2);
  });
});

describe('undoLastBatch', () => {
  it('calls undoBatch with the last batch id and resets to Idle', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);
    vi.mocked(fileIpc.executeOperations).mockResolvedValue({
      status: 'ok',
      data: okExecute('b1', ['a']),
    });
    await useClassifyStore.getState().execute();
    expect(useClassifyStore.getState().lastBatchId).toBe('b1');

    vi.mocked(fileIpc.undoBatch).mockResolvedValue({
      status: 'ok',
      data: { success: true, undone_count: 1, failed_count: 0, task_id: 't1' },
    });
    await useClassifyStore.getState().undoLastBatch();

    expect(fileIpc.undoBatch).toHaveBeenCalledWith('b1');
    expect(useClassifyStore.getState().status).toBe(ClassifyStatus.Idle);
    expect(useClassifyStore.getState().lastBatchId).toBeNull();
  });

  it('sets error when no batch id exists', async () => {
    await useClassifyStore.getState().undoLastBatch();
    expect(fileIpc.undoBatch).not.toHaveBeenCalled();
    expect(useClassifyStore.getState().error).toBe('无最近批次可撤销');
  });
});

describe('reset', () => {
  it('clears preview, progress and summary', async () => {
    const items = [makeItem('a')];
    vi.mocked(fileIpc.classifyPreview).mockResolvedValue({
      status: 'ok',
      data: makePreview(items),
    });
    await useClassifyStore.getState().generatePreview(['a']);

    useClassifyStore.getState().reset();

    const state = useClassifyStore.getState();
    expect(state.status).toBe(ClassifyStatus.Idle);
    expect(state.preview).toBeNull();
    expect(state.pendingIds).toEqual([]);
    expect(state.progress).toEqual({ done: 0, total: 0 });
    expect(state.execSummary).toBeNull();
    expect(state.error).toBeNull();
  });
});
