// StepConsent（引导页同意书）滚动到底门控测试。
//
// 与 CloudConsentDialog.test.tsx 相同策略：mock ConsentAgreement 为可控组件，
// 精确验证「未滚动到底 checkbox 禁用 → 滚动到底可勾选 → 确认」。

import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StepConsent } from './StepConsent';

vi.mock('../consent/ConsentAgreement', () => ({
  ConsentAgreement: (props: { version: string; onBottomReached: (reached: boolean) => void }) => (
    <div>
      <button type="button" onClick={() => props.onBottomReached(true)}>
        scroll-to-bottom
      </button>
    </div>
  ),
}));

describe('StepConsent', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('keeps checkbox and confirm disabled until scrolled to bottom', () => {
    render(<StepConsent onConfirm={vi.fn()} onBack={vi.fn()} />);
    const checkbox = screen.getByRole('checkbox');
    const confirm = screen.getByRole('button', { name: '确认并继续' });
    expect(checkbox).toBeDisabled();
    expect(confirm).toBeDisabled();

    fireEvent.click(screen.getByText('scroll-to-bottom'));
    expect(checkbox).not.toBeDisabled();
    expect(confirm).toBeDisabled(); // 仍未勾选
  });

  it('enables confirm after scrolling and checking', async () => {
    const user = userEvent.setup();
    render(<StepConsent onConfirm={vi.fn()} onBack={vi.fn()} />);
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    expect(screen.getByRole('button', { name: '确认并继续' })).not.toBeDisabled();
  });

  it('calls onConfirm with the selected provider and onBack from the back button', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const onBack = vi.fn();
    render(<StepConsent onConfirm={onConfirm} onBack={onBack} />);
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: '确认并继续' }));
    expect(onConfirm).toHaveBeenCalledWith('Openai');

    fireEvent.click(screen.getByRole('button', { name: '← 返回选其他模式' }));
    expect(onBack).toHaveBeenCalled();
  });
});
