// CloudConsentDialog 滚动到底门控测试。
//
// mock ConsentAgreement 为「点击按钮触发 onBottomReached(true)」的控制组件，
// 使测试能精确模拟「未滚动到底 → checkbox 禁用 / 滚动到底 → 可勾选」。
// 真实滚动检测逻辑由 ConsentAgreement.test.tsx 覆盖。

import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudConsentDialog } from './CloudConsentDialog';

vi.mock('../consent/ConsentAgreement', () => ({
  ConsentAgreement: (props: { version: string; onBottomReached: (reached: boolean) => void }) => (
    <div>
      <button type="button" onClick={() => props.onBottomReached(true)}>
        scroll-to-bottom
      </button>
    </div>
  ),
}));

describe('CloudConsentDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('keeps checkbox and confirm disabled until scrolled to bottom', () => {
    render(<CloudConsentDialog busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
    const checkbox = screen.getByRole('checkbox');
    const confirm = screen.getByRole('button', { name: '确认并切换到云端' });
    // 未滚动到底：checkbox 禁用（不可勾选），确认自然禁用
    expect(checkbox).toBeDisabled();
    expect(confirm).toBeDisabled();

    fireEvent.click(screen.getByText('scroll-to-bottom'));
    // 滚动到底后 checkbox 可用；但未勾选时确认仍禁用
    expect(checkbox).not.toBeDisabled();
    expect(confirm).toBeDisabled();
  });

  it('enables confirm after scrolling and checking the agreement', async () => {
    const user = userEvent.setup();
    render(<CloudConsentDialog busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    expect(screen.getByRole('button', { name: '确认并切换到云端' })).not.toBeDisabled();
  });

  it('calls onConfirm with the selected provider', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<CloudConsentDialog busy={false} onConfirm={onConfirm} onCancel={vi.fn()} />);
    fireEvent.click(screen.getByText('scroll-to-bottom'));
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: '确认并切换到云端' }));
    expect(onConfirm).toHaveBeenCalledWith('Openai');
  });

  it('renders the consent title', () => {
    render(<CloudConsentDialog busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByText('隐私知情同意书')).toBeInTheDocument();
  });
});
