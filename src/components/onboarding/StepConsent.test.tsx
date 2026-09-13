// StepConsent（引导页同意书）滚动到底门控测试。
//
// 与 CloudConsentDialog.test.tsx 相同策略：mock ConsentAgreement 为可控组件，
// 精确验证「未滚动到底 checkbox 禁用 → 滚动到底可勾选 → 确认」。

import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../../stores/settingsStore';
import type { CloudProviderRecord } from '../../types/ipc';
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

// P-07：默认选中 provider = cloudProviders[0]，seed 使默认键为 'Openai'（既有断言大小写）
const builtinCloudProviders: CloudProviderRecord[] = [
  {
    id: 'builtin-openai',
    provider_key: 'Openai',
    name: 'OpenAI',
    remark: '',
    website: null,
    base_url: '',
    is_builtin: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'builtin-deepseek',
    provider_key: 'Deepseek',
    name: 'DeepSeek',
    remark: '',
    website: null,
    base_url: '',
    is_builtin: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

describe('StepConsent', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      cloudProviders: builtinCloudProviders,
      cloudProvidersLoading: false,
    });
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
