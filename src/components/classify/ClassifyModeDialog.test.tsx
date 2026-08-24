// ClassifyModeDialog 单元测试：渲染两个选项，点击回调对应分类方式。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ClassifyModeDialog } from './ClassifyModeDialog';

describe('ClassifyModeDialog', () => {
  it('renders move and copy options', () => {
    render(<ClassifyModeDialog onChoose={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByText('移动分类')).toBeInTheDocument();
    expect(screen.getByText('复制分类')).toBeInTheDocument();
    expect(screen.getByText('取消')).toBeInTheDocument();
  });

  it('calls onChoose with move when move option clicked', async () => {
    const user = userEvent.setup();
    const onChoose = vi.fn();
    render(<ClassifyModeDialog onChoose={onChoose} onCancel={vi.fn()} />);

    await user.click(screen.getByText('移动分类'));

    expect(onChoose).toHaveBeenCalledWith('move');
  });

  it('calls onChoose with copy when copy option clicked', async () => {
    const user = userEvent.setup();
    const onChoose = vi.fn();
    render(<ClassifyModeDialog onChoose={onChoose} onCancel={vi.fn()} />);

    await user.click(screen.getByText('复制分类'));

    expect(onChoose).toHaveBeenCalledWith('copy');
  });

  it('calls onCancel when cancel clicked', async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<ClassifyModeDialog onChoose={vi.fn()} onCancel={onCancel} />);

    await user.click(screen.getByText('取消'));

    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
