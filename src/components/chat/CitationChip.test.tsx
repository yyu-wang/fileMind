// CitationChip 单元测试：编号/文件名/页码渲染与点击回调。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { CitationChip } from './CitationChip';

describe('CitationChip', () => {
  it('renders index, file name and page', () => {
    render(
      <CitationChip
        citation={{ id: 3, fileName: 'a.pdf', page: 2, text: 'x' }}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText('[3]')).toBeInTheDocument();
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
    expect(screen.getByText('P2')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('title', 'a.pdf 第 2 页');
  });

  it('fires onClick when clicked', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      <CitationChip citation={{ id: 1, fileName: 'b.md', page: 0, text: '' }} onClick={onClick} />,
    );
    await user.click(screen.getByRole('button'));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
