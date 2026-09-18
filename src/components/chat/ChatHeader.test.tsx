// ChatHeader 单元测试：计数展示、索引按钮状态与提示、清空对话显隐与回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ChatHeader } from './ChatHeader';

function renderHeader(overrides: Partial<Parameters<typeof ChatHeader>[0]> = {}) {
  const props = {
    totalFiles: 42,
    showClearHistory: false,
    index: { building: false, message: null, ready: true, onBuild: vi.fn() },
    onClearHistory: vi.fn(),
    ...overrides,
  };
  render(<ChatHeader {...props} />);
  return props;
}

describe('ChatHeader', () => {
  it('展示标题与已索引文件数', () => {
    renderHeader();
    expect(screen.getByText('💬 知识问答')).toBeInTheDocument();
    expect(screen.getByText('基于 42 个已索引文件')).toBeInTheDocument();
  });

  it('索引进行中时显示进行中文案并禁用', () => {
    renderHeader({ index: { building: true, message: null, ready: true, onBuild: vi.fn() } });
    const button = screen.getByTestId('build-index');
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent('索引中…');
  });

  it('引擎未就绪时禁用并给出原因', () => {
    renderHeader({ index: { building: false, message: null, ready: false, onBuild: vi.fn() } });
    const button = screen.getByTestId('build-index');
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('title', 'AI 引擎未就绪，暂时无法建立索引');
  });

  it('点击建索引上报意图', async () => {
    const user = userEvent.setup();
    const props = renderHeader();

    await user.click(screen.getByTestId('build-index'));

    expect(props.index.onBuild).toHaveBeenCalledTimes(1);
  });

  it('索引结果提示按 message 显隐', () => {
    renderHeader({
      index: {
        building: false,
        message: '索引完成：10 个文件，跳过 2 个',
        ready: true,
        onBuild: vi.fn(),
      },
    });
    expect(screen.getByRole('status')).toHaveTextContent('索引完成：10 个文件，跳过 2 个');
  });

  it('有消息时显示清空对话并可点击', async () => {
    const user = userEvent.setup();
    const props = renderHeader({ showClearHistory: true });

    await user.click(screen.getByRole('button', { name: '🧹 清空对话' }));

    expect(props.onClearHistory).toHaveBeenCalledTimes(1);
  });

  it('无消息时不渲染清空对话', () => {
    renderHeader({ showClearHistory: false });
    expect(screen.queryByRole('button', { name: '🧹 清空对话' })).not.toBeInTheDocument();
  });
});
