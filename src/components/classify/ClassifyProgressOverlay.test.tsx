// ClassifyProgressOverlay 单元测试：进度渲染、暂停/继续/取消回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ClassifyStatus } from '@/types/models';
import { ClassifyProgressOverlay } from './ClassifyProgressOverlay';

function renderOverlay(status: ClassifyStatus) {
  const props = {
    progress: { done: 3, total: 10 },
    status,
    onPause: vi.fn(),
    onResume: vi.fn(),
    onCancel: vi.fn(),
  };
  render(<ClassifyProgressOverlay {...props} />);
  return props;
}

describe('ClassifyProgressOverlay', () => {
  it('renders progress count and percent', () => {
    renderOverlay(ClassifyStatus.Running);
    expect(screen.getByText('3/10')).toBeInTheDocument();
    expect(screen.getByText('30%')).toBeInTheDocument();
    expect(screen.getByText('正在分类…')).toBeInTheDocument();
  });

  it('shows 暂停 button in running state and triggers onPause', async () => {
    const user = userEvent.setup();
    const props = renderOverlay(ClassifyStatus.Running);
    const pauseBtn = screen.getByRole('button', { name: '暂停' });
    await user.click(pauseBtn);
    expect(props.onPause).toHaveBeenCalledTimes(1);
  });

  it('shows 继续 button in paused state and triggers onResume', async () => {
    const user = userEvent.setup();
    const props = renderOverlay(ClassifyStatus.Paused);
    expect(screen.getByText('已暂停')).toBeInTheDocument();
    const resumeBtn = screen.getByRole('button', { name: '继续' });
    await user.click(resumeBtn);
    expect(props.onResume).toHaveBeenCalledTimes(1);
  });

  it('triggers onCancel when 取消 is clicked', async () => {
    const user = userEvent.setup();
    const props = renderOverlay(ClassifyStatus.Running);
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('avoids division by zero when total is 0', () => {
    render(
      <ClassifyProgressOverlay
        progress={{ done: 0, total: 0 }}
        status={ClassifyStatus.Running}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText('0%')).toBeInTheDocument();
  });
});
