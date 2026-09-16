// ClassifyIntroPanel 单元测试：startButton 文案/禁用态透传、开始与历史回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ClassifyIntroPanel } from './ClassifyIntroPanel';

function renderPanel(overrides: Partial<Parameters<typeof ClassifyIntroPanel>[0]> = {}) {
  const props = {
    startButton: { label: '全部分类', disabled: false },
    onStart: vi.fn(),
    onOpenHistory: vi.fn(),
    ...overrides,
  };
  render(<ClassifyIntroPanel {...props} />);
  return props;
}

describe('ClassifyIntroPanel', () => {
  it('渲染说明与开始按钮文案', () => {
    renderPanel();
    expect(screen.getByText('按规则与文件类型自动整理')).toBeInTheDocument();
    expect(screen.getByText(/待确认/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '全部分类' })).toBeEnabled();
  });

  it('startButton.disabled 透传到按钮', () => {
    renderPanel({ startButton: { label: '请先在文件页扫描目录', disabled: true } });
    expect(screen.getByRole('button', { name: '请先在文件页扫描目录' })).toBeDisabled();
  });

  it('点击开始与查看历史分别上报', async () => {
    const user = userEvent.setup();
    const props = renderPanel();

    await user.click(screen.getByRole('button', { name: '全部分类' }));
    expect(props.onStart).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: '查看分类历史' }));
    expect(props.onOpenHistory).toHaveBeenCalledTimes(1);
  });
});
