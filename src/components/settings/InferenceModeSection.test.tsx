// InferenceModeSection 测试：mode-selector 结构 + 本地/云端模式切换入口 + 一键撤回联动。
//
// fileIpc 打桩，store action 为真实实现（与 CloudApiKeySection.test 同模式）；
// 验证 Cloud 模式展示已签署信息、点「撤回并切回本地」调 revokeCloudConsent
// 并自动切回本地。

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../lib/ipc', () => ({
  fileIpc: {
    signCloudConsent: vi.fn(),
    revokeCloudConsent: vi.fn(),
  },
}));

import { fileIpc } from '../../lib/ipc';
import { useSettingsStore } from '../../stores/settingsStore';
import { InferenceModeSection } from './InferenceModeSection';

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({
    inferenceMode: 'Local',
    cloudConsentSigned: false,
    cloudConsentVersion: null,
    cloudConsentProvider: null,
    cloudConsentSignedAt: null,
    error: null,
  });
});

describe('InferenceModeSection', () => {
  it('renders local and cloud mode-options with local selected by default', () => {
    render(<InferenceModeSection />);
    expect(screen.getByTestId('mode-option-local')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('mode-option-cloud')).toHaveAttribute('aria-checked', 'false');
    // 本地 mode-option 的 aria-label 应包含「本地模式（当前）」
    expect(screen.getByTestId('mode-option-local')).toHaveAccessibleName(/本地模式（当前）/);
  });

  it('does not render hybrid mode-option (P1 unsupported)', () => {
    render(<InferenceModeSection />);
    // P1 未开发功能直接隐藏，不渲染混合模式占位选项
    expect(screen.queryByTestId('mode-option-hybrid')).not.toBeInTheDocument();
  });

  it('opens the consent dialog when clicking cloud mode-option from local', async () => {
    const user = userEvent.setup();
    render(<InferenceModeSection />);
    await user.click(screen.getByTestId('mode-option-cloud'));
    expect(screen.getByRole('dialog', { name: '隐私知情同意书' })).toBeInTheDocument();
  });

  it('marks cloud mode-option as selected in cloud mode and shows revoke button', () => {
    useSettingsStore.setState({
      inferenceMode: 'Cloud',
      cloudConsentSigned: true,
      cloudConsentVersion: 'v1.0',
      cloudConsentProvider: 'Deepseek',
      cloudConsentSignedAt: '2026-08-23T10:00:00.000Z',
    });
    render(<InferenceModeSection />);

    expect(screen.getByTestId('mode-option-cloud')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('mode-option-local')).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByRole('button', { name: '撤回并切回本地' })).toBeInTheDocument();
    const info = screen.getByText(/已签署同意书/);
    expect(info.textContent).toContain('Deepseek');
    expect(info.textContent).toContain('v1.0');
  });

  it('revokes consent and switches back to local mode when clicking local from cloud', async () => {
    useSettingsStore.setState({
      inferenceMode: 'Cloud',
      cloudConsentSigned: true,
      cloudConsentVersion: 'v1.0',
      cloudConsentProvider: 'Openai',
      cloudConsentSignedAt: '2026-08-23T10:00:00.000Z',
    });
    vi.mocked(fileIpc.revokeCloudConsent).mockResolvedValue({
      status: 'ok',
      data: { success: true, switched_to: 'local' },
    });
    const user = userEvent.setup();
    render(<InferenceModeSection />);

    await user.click(screen.getByTestId('mode-option-local'));

    expect(fileIpc.revokeCloudConsent).toHaveBeenCalled();
    // 04 API §2-3d 联动：撤回后自动切回本地，同意信息消失
    await screen.findByTestId('mode-option-local');
    expect(screen.getByTestId('mode-option-local')).toHaveAttribute('aria-checked', 'true');
    expect(screen.queryByText(/已签署同意书/)).not.toBeInTheDocument();
  });
});
