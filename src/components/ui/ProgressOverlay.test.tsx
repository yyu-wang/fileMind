// ProgressOverlay 通用进度遮罩测试：渲染、百分比计算、未开态不渲染。

import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';

import { ProgressOverlay } from './ProgressOverlay';

describe('ProgressOverlay', () => {
  it('returns null when closed', () => {
    const { container } = render(<ProgressOverlay open={false} title="x" total={10} current={0} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders title, count and percent', () => {
    render(
      <ProgressOverlay open title="正在整理文件..." total={1234} current={567} ok={560} fail={7} />,
    );
    expect(screen.getByRole('heading', { level: 3 })).toHaveTextContent('正在整理文件...');
    expect(screen.getByText('567 / 1,234')).toBeInTheDocument();
    expect(screen.getByText('45%')).toBeInTheDocument();
    expect(screen.getByText(/✓ 成功 560/)).toBeInTheDocument();
    expect(screen.getByText(/⚠️ 失败 7/)).toBeInTheDocument();
  });

  it('clamps percent to 100 when current exceeds total', () => {
    render(<ProgressOverlay open title="t" total={10} current={20} />);
    expect(screen.getByText('100%')).toBeInTheDocument();
  });

  it('handles zero total without NaN', () => {
    render(<ProgressOverlay open title="t" total={0} current={0} />);
    expect(screen.getByText('0 / 0')).toBeInTheDocument();
    expect(screen.getByText('0%')).toBeInTheDocument();
  });
});
