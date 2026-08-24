// InferenceModeSection 测试：本地/云端模式切换入口 + 一键撤回联动。
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
  it('opens the consent dialog from local mode', async () => {
    const user = userEvent.setup();
    render(<InferenceModeSection />);
    expect(screen.getByText('🛡️ 本地模式')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '切换到云端' }));

    expect(screen.getByRole('dialog', { name: '隐私知情同意书' })).toBeInTheDocument();
  });

  it('shows revoke button and signed consent info in cloud mode', () => {
    useSettingsStore.setState({
      inferenceMode: 'Cloud',
      cloudConsentSigned: true,
      cloudConsentVersion: 'v1.0',
      cloudConsentProvider: 'Deepseek',
      cloudConsentSignedAt: '2026-08-23T10:00:00.000Z',
    });
    render(<InferenceModeSection />);

    expect(screen.getByText('☁️ 云端模式')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '撤回并切回本地' })).toBeInTheDocument();
    const info = screen.getByText(/已签署同意书/);
    expect(info.textContent).toContain('Deepseek');
    expect(info.textContent).toContain('v1.0');
  });

  it('revokes consent and switches back to local mode', async () => {
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

    await user.click(screen.getByRole('button', { name: '撤回并切回本地' }));

    expect(fileIpc.revokeCloudConsent).toHaveBeenCalled();
    // 04 API §2-3d 联动：撤回后自动切回本地，同意信息消失
    await screen.findByText('🛡️ 本地模式');
    expect(screen.queryByText(/已签署同意书/)).not.toBeInTheDocument();
  });
});
