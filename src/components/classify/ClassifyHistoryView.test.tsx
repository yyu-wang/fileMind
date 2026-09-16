// ClassifyHistoryView 单元测试：挂载拉取列表、撤销二次确认链路、批次明细切换。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { useClassifyHistoryStore } from '@/stores/classifyHistoryStore';

import { ClassifyHistoryView } from './ClassifyHistoryView';

const mocks = vi.hoisted(() => ({
  getOperationHistory: vi.fn(),
  getBatchDetail: vi.fn(),
  undoBatch: vi.fn(),
  getFileStats: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    getOperationHistory: mocks.getOperationHistory,
    getBatchDetail: mocks.getBatchDetail,
    undoBatch: mocks.undoBatch,
    getFileStats: mocks.getFileStats,
  },
}));

// 批次明细内的懒加载预览抽屉会牵出 react-pdf：与用例无关，替换为空桩
vi.mock('@/components/common/LazyFilePreviewDrawer', () => ({
  LazyFilePreviewDrawer: () => null,
}));

const batches = [
  {
    batch_id: 'b1',
    op_type: 'move',
    total_count: 2,
    success_count: 2,
    failed_count: 0,
    status: 'done',
    created_at: '2026-01-01 00:00:00',
    has_delete: false,
    can_undo: true,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  useClassifyHistoryStore.setState({
    batches: [],
    detail: null,
    loading: false,
    undoing: false,
    error: null,
  });
  mocks.getOperationHistory.mockResolvedValue({
    status: 'ok',
    data: { batches, page: 1, page_size: 20 },
  });
  mocks.getBatchDetail.mockResolvedValue({
    status: 'ok',
    data: { batch_id: 'b1', logs: [] },
  });
  mocks.undoBatch.mockResolvedValue({ status: 'ok', data: null });
  mocks.getFileStats.mockResolvedValue({ status: 'ok', data: { total_files: 0 } });
});

describe('ClassifyHistoryView', () => {
  it('挂载即拉取并渲染批次列表', async () => {
    render(<ClassifyHistoryView onBack={vi.fn()} />);

    expect(await screen.findByText('分类历史')).toBeInTheDocument();
    expect(mocks.getOperationHistory).toHaveBeenCalledTimes(1);
    expect(screen.getByText('移动 · 2/2 成功')).toBeInTheDocument();
  });

  it('返回按钮上报回退意图', async () => {
    const user = userEvent.setup();
    const onBack = vi.fn();
    render(<ClassifyHistoryView onBack={onBack} />);

    await user.click(await screen.findByRole('button', { name: '返回' }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('撤销需经二次确认后才调用后端', async () => {
    const user = userEvent.setup();
    render(<ClassifyHistoryView onBack={vi.fn()} />);

    await user.click(await screen.findByRole('button', { name: '撤销' }));
    expect(mocks.undoBatch).not.toHaveBeenCalled();
    expect(screen.getByText('撤销该批次？')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '确认撤销' }));
    expect(mocks.undoBatch).toHaveBeenCalledWith('b1');
    expect(screen.queryByText('撤销该批次？')).not.toBeInTheDocument();
  });

  it('打开批次后切换到明细视图', async () => {
    const user = userEvent.setup();
    render(<ClassifyHistoryView onBack={vi.fn()} />);

    await user.click(await screen.findByText('移动 · 2/2 成功'));
    expect(mocks.getBatchDetail).toHaveBeenCalledWith('b1');
    expect(await screen.findByText('批次明细')).toBeInTheDocument();
    expect(screen.queryByText('分类历史')).not.toBeInTheDocument();
  });
});
