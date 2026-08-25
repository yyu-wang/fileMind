// ClassifyDonePanel 单元测试：完成/取消标题、统计数字、未处理兜底与撤销按钮显隐。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ClassifyExecSummary } from '@/stores/classifyStore';

import { ClassifyDonePanel } from './ClassifyDonePanel';

const summary: ClassifyExecSummary = { success: 5, failed: 2, pending: 1, total: 8 };

describe('ClassifyDonePanel', () => {
  it('renders completion title and stat counts', () => {
    render(
      <ClassifyDonePanel
        summary={summary}
        cancelled={false}
        canUndo
        onUndo={vi.fn()}
        onFinish={vi.fn()}
      />,
    );
    expect(screen.getByText('分类完成')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('成功')).toBeInTheDocument();
    expect(screen.getByText('失败')).toBeInTheDocument();
    expect(screen.getByText('待确认')).toBeInTheDocument();
  });

  it('shows 未处理 count when total exceeds processed sum', () => {
    // 11 - 5 - 2 - 1 = 3
    render(
      <ClassifyDonePanel
        summary={{ success: 5, failed: 2, pending: 1, total: 11 }}
        cancelled={false}
        canUndo
        onUndo={vi.fn()}
        onFinish={vi.fn()}
      />,
    );
    expect(screen.getByText('3')).toBeInTheDocument();
    expect(screen.getByText('未处理')).toBeInTheDocument();
  });

  it('omits 未处理 when all files accounted for', () => {
    render(
      <ClassifyDonePanel
        summary={{ success: 8, failed: 0, pending: 2, total: 10 }}
        cancelled={false}
        canUndo
        onUndo={vi.fn()}
        onFinish={vi.fn()}
      />,
    );
    expect(screen.queryByText('未处理')).not.toBeInTheDocument();
  });

  it('shows cancelled title when cancelled', () => {
    render(
      <ClassifyDonePanel summary={summary} cancelled canUndo onUndo={vi.fn()} onFinish={vi.fn()} />,
    );
    expect(screen.getByText('分类已取消（部分文件已处理）')).toBeInTheDocument();
  });

  it('shows undo with 撤销已执行部分 when cancelled and some succeeded', () => {
    render(
      <ClassifyDonePanel summary={summary} cancelled canUndo onUndo={vi.fn()} onFinish={vi.fn()} />,
    );
    expect(screen.getByTestId('classify-undo')).toHaveTextContent('撤销已执行部分');
  });

  it('hides undo button when canUndo is false', () => {
    render(
      <ClassifyDonePanel
        summary={summary}
        cancelled={false}
        canUndo={false}
        onUndo={vi.fn()}
        onFinish={vi.fn()}
      />,
    );
    expect(screen.queryByTestId('classify-undo')).not.toBeInTheDocument();
  });

  it('calls onUndo and onFinish from buttons', async () => {
    const user = userEvent.setup();
    const onUndo = vi.fn();
    const onFinish = vi.fn();
    render(
      <ClassifyDonePanel
        summary={summary}
        cancelled={false}
        canUndo
        onUndo={onUndo}
        onFinish={onFinish}
      />,
    );
    await user.click(screen.getByTestId('classify-undo'));
    expect(onUndo).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: '完成' }));
    expect(onFinish).toHaveBeenCalledTimes(1);
  });
});
