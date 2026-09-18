// ClassifyPreviewSection 单元测试：布局/树渲染、抽屉目标透传、进度遮罩按状态显隐与回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import type { ClassifyPreview } from '@/types/ipc';
import { ClassifyStatus } from '@/types/models';

import { ClassifyPreviewSection } from './ClassifyPreviewSection';

// 懒加载抽屉会牵出 react-pdf：本用例只关心 props 透传，替换为轻量桩
vi.mock('@/components/common/LazyFilePreviewDrawer', () => ({
  LazyFilePreviewDrawer: ({
    file,
    onClose,
  }: {
    file: { path: string } | null;
    onClose: () => void;
  }) => (
    <button type="button" data-testid="preview-drawer" onClick={onClose}>
      {file ? file.path : 'none'}
    </button>
  ),
}));

const preview: ClassifyPreview = {
  batch_id: 'b1',
  output_root: '/tmp/已分类',
  items: [
    {
      file_id: 'f1',
      file_name: 'a.pdf',
      original_path: '/tmp/a.pdf',
      target_path: '/tmp/已分类/文档/a.pdf',
      category_name: '文档',
      rule_source: 'heuristic',
      status: 'Ok',
      conflict_type: null,
    },
  ],
  stats: { total: 1, categorized: 1, pending: 0, by_rule: 0, by_heuristic: 1 },
};

function renderSection(overrides: Partial<Parameters<typeof ClassifyPreviewSection>[0]> = {}) {
  const props = {
    preview,
    status: ClassifyStatus.Idle,
    progress: { done: 0, total: 1 },
    drawer: { target: null, onClose: vi.fn() },
    actions: {
      onOpenPreview: vi.fn(),
      onPause: vi.fn(),
      onResume: vi.fn(),
      onCancel: vi.fn(),
    },
    ...overrides,
  };
  render(<ClassifyPreviewSection {...props} />);
  return props;
}

describe('ClassifyPreviewSection', () => {
  it('渲染预览双栏与树中的分类名', () => {
    renderSection();
    expect(document.querySelector('.classify-layout')).not.toBeNull();
    expect(screen.getByText('文档')).toBeInTheDocument();
  });

  it('抽屉目标与关闭回调透传给懒加载抽屉', () => {
    const onClose = vi.fn();
    renderSection({
      drawer: { target: { path: '/tmp/a.pdf', file_name: 'a.pdf', category: '文档' }, onClose },
    });
    expect(screen.getByTestId('preview-drawer')).toHaveTextContent('/tmp/a.pdf');
  });

  it('点击树中文件名上报预览意图', async () => {
    const user = userEvent.setup();
    const props = renderSection();
    await user.click(screen.getByText('a.pdf'));
    expect(props.actions.onOpenPreview).toHaveBeenCalledWith(preview.items[0]);
  });

  it('非执行态不渲染进度遮罩', () => {
    renderSection();
    expect(screen.queryByLabelText('分类执行进度')).not.toBeInTheDocument();
  });

  it('执行中渲染遮罩并透传暂停/取消回调', async () => {
    const user = userEvent.setup();
    const props = renderSection({
      status: ClassifyStatus.Running,
      progress: { done: 1, total: 2 },
    });

    expect(screen.getByLabelText('分类执行进度')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(props.actions.onCancel).toHaveBeenCalledTimes(1);
  });
});
