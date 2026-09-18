// ClassifyHeader 单元测试：副标题/路径条件渲染、按钮组显隐与三类回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ClassifyHeader } from './ClassifyHeader';

function renderHeader(overrides: Partial<Parameters<typeof ClassifyHeader>[0]> = {}) {
  const props = {
    scanPath: '/tmp/docs',
    showPreviewSubtitle: true,
    showActions: true,
    onCancelPreview: vi.fn(),
    onExecute: vi.fn(),
    ...overrides,
  };
  render(<ClassifyHeader {...props} />);
  return props;
}

describe('ClassifyHeader', () => {
  it('渲染标题、预览副标题与扫描路径', () => {
    renderHeader();
    expect(screen.getByRole('heading', { name: '智能分类' })).toBeInTheDocument();
    expect(screen.getByText('预览分类方案')).toBeInTheDocument();
    expect(screen.getByText('/tmp/docs')).toBeInTheDocument();
  });

  it('无预览时隐藏副标题，未扫描时隐藏路径', () => {
    renderHeader({ showPreviewSubtitle: false, scanPath: null });
    expect(screen.queryByText('预览分类方案')).not.toBeInTheDocument();
    expect(screen.queryByText('/tmp/docs')).not.toBeInTheDocument();
  });

  it('执行中隐藏按钮组', () => {
    renderHeader({ showActions: false });
    expect(screen.queryByRole('button', { name: '取消' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '✓ 确认执行全部' })).not.toBeInTheDocument();
  });

  it('取消与两种执行入口分别上报意图', async () => {
    const user = userEvent.setup();
    const props = renderHeader();

    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(props.onCancelPreview).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: '仅执行无冲突项' }));
    expect(props.onExecute).toHaveBeenLastCalledWith(false);

    await user.click(screen.getByRole('button', { name: '✓ 确认执行全部' }));
    expect(props.onExecute).toHaveBeenLastCalledWith(true);
  });
});
