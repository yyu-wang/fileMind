// ClassifyHistoryList 单元测试：加载/空态、批次摘要、操作类型映射、撤销显隐与回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { OperationBatchSummary } from '@/types/ipc';

import { ClassifyHistoryList } from './ClassifyHistoryList';

function batch(id: string, overrides: Partial<OperationBatchSummary> = {}): OperationBatchSummary {
  return {
    batch_id: id,
    op_type: 'move',
    total_count: 3,
    success_count: 2,
    failed_count: 1,
    status: 'done',
    created_at: '2026-01-01 00:00:00',
    has_delete: false,
    can_undo: true,
    ...overrides,
  };
}

function renderList(overrides: Partial<Parameters<typeof ClassifyHistoryList>[0]> = {}) {
  const props = {
    batches: [
      batch('b1'),
      batch('b2', {
        op_type: 'delete',
        status: 'undone',
        can_undo: false,
        success_count: 0,
        failed_count: 3,
      }),
    ],
    loading: false,
    error: null,
    undoing: false,
    onRefresh: vi.fn(),
    onBack: vi.fn(),
    onOpenBatch: vi.fn(),
    onRequestUndo: vi.fn(),
    ...overrides,
  };
  render(<ClassifyHistoryList {...props} />);
  return props;
}

describe('ClassifyHistoryList', () => {
  it('renders title and batch count subtitle', () => {
    renderList();
    expect(screen.getByText('分类历史')).toBeInTheDocument();
    expect(screen.getByText('最近 2 批整理记录')).toBeInTheDocument();
  });

  it('shows loading status when loading with no batches', () => {
    renderList({ loading: true, batches: [] });
    expect(screen.getByRole('status')).toHaveTextContent('正在加载分类历史…');
  });

  it('shows empty state when no batches', () => {
    renderList({ batches: [] });
    expect(screen.getByText('暂无分类历史')).toBeInTheDocument();
  });

  it('shows error alert when error present', () => {
    renderList({ error: '加载失败' });
    expect(screen.getByRole('alert')).toHaveTextContent('加载失败');
  });

  it('renders batch summary with op label, success count and status tag', () => {
    renderList();
    expect(screen.getByText('移动 · 2/3 成功')).toBeInTheDocument();
    expect(screen.getByText('成功')).toBeInTheDocument();
    expect(screen.getByText('删除 · 0/3 成功')).toBeInTheDocument();
    expect(screen.getByText('已撤销')).toBeInTheDocument();
  });

  it('renders only undoable batches with 撤销 button', () => {
    renderList();
    expect(screen.getAllByRole('button', { name: '撤销' })).toHaveLength(1);
  });

  it('disables undo buttons while undoing', () => {
    renderList({ undoing: true });
    expect(screen.getByRole('button', { name: '撤销' })).toBeDisabled();
  });

  it('calls onOpenBatch when batch row clicked', async () => {
    const user = userEvent.setup();
    const props = renderList();
    await user.click(screen.getByText('移动 · 2/3 成功'));
    expect(props.onOpenBatch).toHaveBeenCalledWith('b1');
  });

  it('calls onRequestUndo from undo button', async () => {
    const user = userEvent.setup();
    const props = renderList();
    await user.click(screen.getByRole('button', { name: '撤销' }));
    expect(props.onRequestUndo).toHaveBeenCalledWith(props.batches[0]);
  });

  it('refresh is disabled while loading', () => {
    renderList({ loading: true, batches: [batch('b1')] });
    expect(screen.getByRole('button', { name: '刷新中…' })).toBeDisabled();
  });

  it('refresh calls onRefresh when not loading', async () => {
    const user = userEvent.setup();
    const props = renderList({ batches: [batch('b1')] });
    await user.click(screen.getByRole('button', { name: '刷新' }));
    expect(props.onRefresh).toHaveBeenCalledTimes(1);
  });

  it('calls onBack from 返回 button', async () => {
    const user = userEvent.setup();
    const props = renderList();
    await user.click(screen.getByRole('button', { name: '返回' }));
    expect(props.onBack).toHaveBeenCalledTimes(1);
  });
});
