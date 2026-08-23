// classifyHistoryStore 单元测试：批次加载、明细打开、整批撤销（含失败路径）。

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    getOperationHistory: vi.fn(),
    getBatchDetail: vi.fn(),
    undoBatch: vi.fn(),
    getFileStats: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import type { BatchDetailResponse, OperationBatchSummary } from '../types/ipc';
import { useClassifyHistoryStore } from './classifyHistoryStore';

function makeBatch(overrides: Partial<OperationBatchSummary> = {}): OperationBatchSummary {
  return {
    batch_id: 'batch-1',
    op_type: 'move',
    total_count: 3,
    success_count: 3,
    failed_count: 0,
    status: 'done',
    created_at: '2026-08-20 10:00:00',
    has_delete: false,
    can_undo: true,
    ...overrides,
  };
}

function makeDetail(batchId: string): BatchDetailResponse {
  return {
    batch_id: batchId,
    logs: [
      {
        id: 'log-1',
        batch_id: batchId,
        operation_type: 'move',
        source_path: '/tmp/root/a.txt',
        target_path: '/tmp/root/文档/a.txt',
        status: 'done',
        prev_hash: 'h1',
        current_hash: 'h2',
        chain_hash: 'c1',
        created_at: '2026-08-20 10:00:00',
      },
    ],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  useClassifyHistoryStore.setState({
    batches: [],
    detail: null,
    loading: false,
    undoing: false,
    error: null,
  });
});

describe('classifyHistoryStore', () => {
  it('loadHistory 成功后填充批次列表', async () => {
    vi.mocked(fileIpc.getOperationHistory).mockResolvedValue({
      status: 'ok',
      data: { batches: [makeBatch()], page: 1, page_size: 20 },
    });

    await useClassifyHistoryStore.getState().loadHistory();

    const s = useClassifyHistoryStore.getState();
    expect(s.batches).toHaveLength(1);
    expect(s.batches[0].batch_id).toBe('batch-1');
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  it('loadHistory 失败时记录错误', async () => {
    vi.mocked(fileIpc.getOperationHistory).mockResolvedValue({
      status: 'error',
      error: '查询失败',
    });

    await useClassifyHistoryStore.getState().loadHistory();

    const s = useClassifyHistoryStore.getState();
    expect(s.batches).toHaveLength(0);
    expect(s.error).toBe('查询失败');
  });

  it('openBatch 成功后填充批次明细', async () => {
    vi.mocked(fileIpc.getBatchDetail).mockResolvedValue({
      status: 'ok',
      data: makeDetail('batch-1'),
    });

    await useClassifyHistoryStore.getState().openBatch('batch-1');

    const s = useClassifyHistoryStore.getState();
    expect(s.detail?.batch_id).toBe('batch-1');
    expect(s.detail?.logs).toHaveLength(1);
    expect(s.loading).toBe(false);
  });

  it('closeBatch 清空明细', () => {
    useClassifyHistoryStore.setState({ detail: makeDetail('batch-1') });

    useClassifyHistoryStore.getState().closeBatch();

    expect(useClassifyHistoryStore.getState().detail).toBeNull();
  });

  it('undoBatch 成功后调用撤销并刷新列表', async () => {
    vi.mocked(fileIpc.undoBatch).mockResolvedValue({
      status: 'ok',
      data: { success: true, undone_count: 3, failed_count: 0, task_id: 'task-1' },
    });
    vi.mocked(fileIpc.getOperationHistory).mockResolvedValue({
      status: 'ok',
      data: { batches: [makeBatch({ status: 'undone', can_undo: false })], page: 1, page_size: 20 },
    });
    vi.mocked(fileIpc.getFileStats).mockResolvedValue({
      status: 'ok',
      data: {
        total_files: 10,
        categorized_files: 5,
        uncategorized_files: 5,
        duplicate_groups: 0,
        total_size_bytes: 1024,
      },
    });

    await useClassifyHistoryStore.getState().undoBatch('batch-1');

    expect(fileIpc.undoBatch).toHaveBeenCalledWith('batch-1');
    // 成功后重新拉取列表（批次状态已变为 undone，can_undo 关闭）
    expect(fileIpc.getOperationHistory).toHaveBeenCalledTimes(1);
    const s = useClassifyHistoryStore.getState();
    expect(s.batches[0].status).toBe('undone');
    expect(s.batches[0].can_undo).toBe(false);
    expect(s.undoing).toBe(false);
  });

  it('undoBatch 失败时记录错误且不刷新', async () => {
    vi.mocked(fileIpc.undoBatch).mockResolvedValue({ status: 'error', error: '撤销失败' });

    await useClassifyHistoryStore.getState().undoBatch('batch-1');

    const s = useClassifyHistoryStore.getState();
    expect(s.error).toBe('撤销失败');
    expect(fileIpc.getOperationHistory).not.toHaveBeenCalled();
    expect(s.undoing).toBe(false);
  });
});
