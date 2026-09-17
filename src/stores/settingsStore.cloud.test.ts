// settingsStore 单元测试（云端）：同意书签署/撤回 + API Key 状态管理。
//
// 从 settingsStore.test.ts 拆出（原单文件 572 行超测试文件警告阈值 400）；
// 共享夹具与 vi.mock 的说明见该文件与本目录 settingsTestFixtures.ts。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    getConfig: vi.fn(),
    setInferenceMode: vi.fn(),
    ollamaStatus: vi.fn(),
    updateConfig: vi.fn(),
    signCloudConsent: vi.fn(),
    revokeCloudConsent: vi.fn(),
    getApiKeyStatus: vi.fn(),
    setApiKey: vi.fn(),
    deleteApiKey: vi.fn(),
    importModelPackage: vi.fn(),
  },
}));

import { CLOUD_CONSENT_VERSION } from '../lib/consent';
import { fileIpc } from '../lib/ipc';
import { useSettingsStore } from './settingsStore';
import { resetSettings } from './settingsTestFixtures';

describe('settingsStore · 云端同意书', () => {
  beforeEach(resetSettings);

  it('signCloudConsent 成功：置同意状态并切云端', async () => {
    (fileIpc.signCloudConsent as Mock).mockResolvedValue({ status: 'ok', data: { success: true } });

    await useSettingsStore.getState().signCloudConsent('Openai');

    const s = useSettingsStore.getState();
    expect(fileIpc.signCloudConsent).toHaveBeenCalledWith(CLOUD_CONSENT_VERSION, 'Openai');
    expect(s.cloudConsentSigned).toBe(true);
    expect(s.inferenceMode).toBe('Cloud');
    expect(s.cloudConsentProvider).toBe('Openai');
    expect(s.cloudConsentVersion).toBe(CLOUD_CONSENT_VERSION);
    expect(s.cloudConsentSignedAt).toEqual(expect.any(String));
  });

  it('signCloudConsent 失败：抛异常', async () => {
    (fileIpc.signCloudConsent as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await expect(useSettingsStore.getState().signCloudConsent('Openai')).rejects.toThrow(
      'DB-U-001',
    );
  });

  it('revokeCloudConsent 成功：清空同意并切回 Local', async () => {
    useSettingsStore.setState({ cloudConsentSigned: true, inferenceMode: 'Cloud' });
    (fileIpc.revokeCloudConsent as Mock).mockResolvedValue({
      status: 'ok',
      data: { success: true, switched_to: 'local' },
    });

    await useSettingsStore.getState().revokeCloudConsent();

    const s = useSettingsStore.getState();
    expect(s.cloudConsentSigned).toBe(false);
    expect(s.inferenceMode).toBe('Local');
    expect(s.cloudConsentVersion).toBeNull();
    expect(s.cloudConsentProvider).toBeNull();
    expect(s.cloudConsentSignedAt).toBeNull();
  });

  it('revokeCloudConsent 失败：抛异常', async () => {
    (fileIpc.revokeCloudConsent as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await expect(useSettingsStore.getState().revokeCloudConsent()).rejects.toThrow('DB-U-001');
  });
});

describe('settingsStore · API Key 状态', () => {
  beforeEach(resetSettings);

  it('loadApiKeyStatus 成功：数组映射为 provider 键', async () => {
    (fileIpc.getApiKeyStatus as Mock).mockResolvedValue({
      status: 'ok',
      data: [
        { provider: 'Openai', has_key: true, hint: '····abcd' },
        { provider: 'Deepseek', has_key: false, hint: '' },
      ],
    });

    await useSettingsStore.getState().loadApiKeyStatus();

    const m = useSettingsStore.getState().apiKeyStatus;
    expect(m.Openai).toEqual({ provider: 'Openai', has_key: true, hint: '····abcd' });
    expect(m.Deepseek.has_key).toBe(false);
  });

  it('loadApiKeyStatus 失败：置错', async () => {
    (fileIpc.getApiKeyStatus as Mock).mockResolvedValue({ status: 'error', error: 'KEY-E-001' });

    await useSettingsStore.getState().loadApiKeyStatus();

    expect(useSettingsStore.getState().error).toBe('KEY-E-001');
  });

  it('FE-M12: 部分返回 → 其余 provider 保留默认而非 undefined', async () => {
    (fileIpc.getApiKeyStatus as Mock).mockResolvedValue({
      status: 'ok',
      data: [{ provider: 'Openai', has_key: true, hint: '***abcd' }],
    });

    await useSettingsStore.getState().loadApiKeyStatus();

    const map = useSettingsStore.getState().apiKeyStatus;
    expect(map.Openai.has_key).toBe(true);
    expect(map.Deepseek).toEqual({ provider: 'Deepseek', has_key: false, hint: '' });
  });

  it('setApiKey 成功：仅更新该服务商状态', async () => {
    (fileIpc.setApiKey as Mock).mockResolvedValue({
      status: 'ok',
      data: { provider: 'Deepseek', has_key: true, hint: '····wxyz' },
    });

    await useSettingsStore.getState().setApiKey('Deepseek', 'sk-xxx');

    const m = useSettingsStore.getState().apiKeyStatus;
    expect(m.Deepseek.hint).toBe('····wxyz');
    expect(m.Openai.has_key).toBe(false); // 其它服务商不受影响
  });

  it('setApiKey 失败：抛异常', async () => {
    (fileIpc.setApiKey as Mock).mockResolvedValue({ status: 'error', error: 'KEY-INVALID' });

    await expect(useSettingsStore.getState().setApiKey('Openai', 'bad')).rejects.toThrow(
      'KEY-INVALID',
    );
  });

  it('deleteApiKey 成功：更新为未配置', async () => {
    (fileIpc.deleteApiKey as Mock).mockResolvedValue({
      status: 'ok',
      data: { provider: 'Openai', has_key: false, hint: '' },
    });

    await useSettingsStore.getState().deleteApiKey('Openai');

    expect(useSettingsStore.getState().apiKeyStatus.Openai.has_key).toBe(false);
  });

  it('deleteApiKey 失败：抛异常', async () => {
    (fileIpc.deleteApiKey as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await expect(useSettingsStore.getState().deleteApiKey('Openai')).rejects.toThrow('DB-U-001');
  });
});
