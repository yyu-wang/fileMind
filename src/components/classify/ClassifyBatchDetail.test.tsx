// ClassifyBatchDetail 单元测试：明细条目、文件名 basename（/ 与 \）、操作类型与源→目标路径。
// 预览入口：done + 可预览操作类型显示「预览」按钮，点击把目标文件交给公共预览抽屉。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { OperationLog } from '@/types/ipc';

import { ClassifyBatchDetail } from './ClassifyBatchDetail';

vi.mock('@/components/common/LazyFilePreviewDrawer', () => ({
  LazyFilePreviewDrawer: ({ file }: { file: { path: string } | null }) =>
    file ? <div data-testid="preview-drawer">{file.path}</div> : null,
}));

function log(overrides: Partial<OperationLog> = {}): OperationLog {
  return {
    id: 'l1',
    batch_id: 'b1',
    operation_type: 'move',
    source_path: '/root/a.pdf',
    target_path: '/root/文档/a.pdf',
    status: 'done',
    prev_hash: '',
    current_hash: '',
    chain_hash: '',
    created_at: 'invalid-ts',
    ...overrides,
  };
}

describe('ClassifyBatchDetail', () => {
  it('renders title, count and first-log timestamp', () => {
    render(<ClassifyBatchDetail logs={[log()]} onBack={vi.fn()} />);
    expect(screen.getByText('批次明细')).toBeInTheDocument();
    // created_at 为无效 UTC 时间 → formatDateTime 原样返回
    expect(screen.getByText('invalid-ts · 1 条操作')).toBeInTheDocument();
  });

  it('renders basename from posix path', () => {
    render(<ClassifyBatchDetail logs={[log()]} onBack={vi.fn()} />);
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
  });

  it('renders basename from windows path', () => {
    render(
      <ClassifyBatchDetail logs={[log({ source_path: 'C:\\docs\\报告.pdf' })]} onBack={vi.fn()} />,
    );
    expect(screen.getByText('报告.pdf')).toBeInTheDocument();
  });

  it('maps operation type and renders status tag', () => {
    render(
      <ClassifyBatchDetail
        logs={[
          log({ operation_type: 'move' }),
          log({ id: 'l2', operation_type: 'rename', status: 'failed' }),
          log({ id: 'l3', operation_type: 'delete' }),
        ]}
        onBack={vi.fn()}
      />,
    );
    expect(screen.getAllByText('移动')).toHaveLength(1);
    expect(screen.getByText('重命名')).toBeInTheDocument();
    expect(screen.getByText('删除')).toBeInTheDocument();
    expect(screen.getByText('失败')).toBeInTheDocument();
  });

  it('renders source → target paths', () => {
    render(<ClassifyBatchDetail logs={[log()]} onBack={vi.fn()} />);
    expect(screen.getByText('/root/a.pdf')).toBeInTheDocument();
    expect(screen.getByText('/root/文档/a.pdf')).toBeInTheDocument();
    expect(screen.getByText('→')).toBeInTheDocument();
  });

  it('maps copy to 复制', () => {
    render(<ClassifyBatchDetail logs={[log({ operation_type: 'copy' })]} onBack={vi.fn()} />);
    expect(screen.getByText('复制')).toBeInTheDocument();
  });

  it('calls onBack from 返回列表 button', async () => {
    const user = userEvent.setup();
    const onBack = vi.fn();
    render(<ClassifyBatchDetail logs={[log()]} onBack={onBack} />);
    await user.click(screen.getByRole('button', { name: '返回列表' }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('hides 预览 for non-previewable states', () => {
    render(
      <ClassifyBatchDetail
        logs={[
          log({ id: 'l1', operation_type: 'delete' }), // 删除成功：目标已不在原路径
          log({ id: 'l2', status: 'undone' }), // 已撤销：还原后无固定预览目标
          log({ id: 'l3', operation_type: 'copy', target_path: '' }), // 成功但无目标路径
          log({ id: 'l4', status: 'failed' }), // 失败：源/目标路径可能已失效
        ]}
        onBack={vi.fn()}
      />,
    );
    expect(screen.queryByRole('button', { name: '预览' })).not.toBeInTheDocument();
  });

  it('shows 预览 only for done rows, never for failed rows', () => {
    render(
      <ClassifyBatchDetail
        logs={[
          log(), // move + done → 预览目标文件
          log({ id: 'l2', status: 'failed' }), // 失败 → 不显示预览
        ]}
        onBack={vi.fn()}
      />,
    );
    expect(screen.getAllByRole('button', { name: '预览' })).toHaveLength(1);
  });

  it('opens preview of the target file for a done row', async () => {
    const user = userEvent.setup();
    render(<ClassifyBatchDetail logs={[log()]} onBack={vi.fn()} />);
    await user.click(screen.getByRole('button', { name: '预览' }));
    expect(screen.getByTestId('preview-drawer')).toHaveTextContent('/root/文档/a.pdf');
  });

  it('hides the drawer when no preview is opened', () => {
    render(<ClassifyBatchDetail logs={[log()]} onBack={vi.fn()} />);
    expect(screen.queryByTestId('preview-drawer')).not.toBeInTheDocument();
  });
});
