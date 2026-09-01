// settingsStore 单元测试：配置加载/推理模式切换/Ollama 探测/云端同意书/API Key/主题。

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
  },
}));

import { CLOUD_CONSENT_VERSION } from '../lib/consent';
import { fileIpc } from '../lib/ipc';
import { ThemeMode } from '../types/models';
import type { AppConfig, OllamaStatus } from '../types/ipc';
import { useSettingsStore } from './settingsStore';

const baseConfig: AppConfig = {
  data_directory: '/data',
  inference_mode: 'local',
  embedding_model: 'bge-small-zh',
  llm_model: 'qwen3.8-27b',
  max_file_size_mb: 100,
  language: 'zh-CN',
  onboarding_completed: false,
  cloud_consent_signed: false,
  cloud_consent_version: null,
  cloud_consent_provider: null,
  cloud_consent_signed_at: null,
};

const ollamaOk: OllamaStatus = {
  available: true,
  status: 'ok',
  llm_models: [{ name: 'qwen3.8-27b', size_bytes: 10, family: 'qwen', modified_at: null }],
  embedding_models: [{ name: 'bge-small-zh', dim: 512, version: 1, available: true }],
  error_code: null,
  message: null,
};

function resetSettings(): void {
  localStorage.clear();
  vi.clearAllMocks();
  useSettingsStore.setState({
    inferenceMode: 'Local',
    llmModel: 'qwen3.8-27b',
    dataDirectory: '',
    embeddingModel: 'bge-small-zh',
    maxFileSizeMb: 100,
    language: 'zh-CN',
    onboardingCompleted: false,
    cloudConsentSigned: false,
    cloudConsentVersion: null,
    cloudConsentProvider: null,
    cloudConsentSignedAt: null,
    isLoading: true,
    initFailed: false,
    error: null,
    ollamaStatus: null,
    ollamaProbing: false,
    lastOllamaProbeAt: 0,
    llmModelOptions: [],
    embeddingModelOptions: [],
    theme: ThemeMode.System,
    apiKeyStatus: {
      Openai: { provider: 'Openai', has_key: false, hint: '' },
      Deepseek: { provider: 'Deepseek', has_key: false, hint: '' },
    },
  });
}

describe('settingsStore', () => {
  beforeEach(resetSettings);

  it('loadConfig 成功：填充全部字段并归一化 cloud 模式', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({
      status: 'ok',
      data: { ...baseConfig, inference_mode: 'cloud', onboarding_completed: true },
    });

    await useSettingsStore.getState().loadConfig();

    const s = useSettingsStore.getState();
    expect(s.isLoading).toBe(false);
    expect(s.dataDirectory).toBe('/data');
    expect(s.inferenceMode).toBe('Cloud');
    expect(s.onboardingCompleted).toBe(true);
    expect(s.error).toBeNull();
  });

  it('loadConfig 未知模式兜底为 Local', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({
      status: 'ok',
      data: { ...baseConfig, inference_mode: 'hybrid' },
    });

    await useSettingsStore.getState().loadConfig();

    expect(useSettingsStore.getState().inferenceMode).toBe('Local');
  });

  it('loadConfig 失败：置错', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'error', error: 'DB-U-001' });

    await useSettingsStore.getState().loadConfig();

    const s = useSettingsStore.getState();
    expect(s.isLoading).toBe(false);
    expect(s.error).toBe('DB-U-001');
  });

  it('setInferenceMode 成功：写入模式并带 ui 来源标记', async () => {
    (fileIpc.setInferenceMode as Mock).mockResolvedValue({ status: 'ok', data: 'Cloud' });

    await useSettingsStore.getState().setInferenceMode('Cloud');

    expect(fileIpc.setInferenceMode).toHaveBeenCalledWith('Cloud', 'ui');
    expect(useSettingsStore.getState().inferenceMode).toBe('Cloud');
  });

  it('setInferenceMode 失败：置错并抛异常（安全阀门）', async () => {
    (fileIpc.setInferenceMode as Mock).mockResolvedValue({
      status: 'error',
      error: 'MODE-FORBIDDEN',
    });

    await expect(useSettingsStore.getState().setInferenceMode('Cloud')).rejects.toThrow(
      'MODE-FORBIDDEN',
    );
    expect(useSettingsStore.getState().error).toBe('MODE-FORBIDDEN');
  });

  it('probeOllama 成功：写入状态与模型选项', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();

    const s = useSettingsStore.getState();
    expect(s.ollamaProbing).toBe(false);
    expect(s.ollamaStatus).toEqual(ollamaOk);
    expect(s.llmModelOptions).toEqual(ollamaOk.llm_models);
    expect(s.embeddingModelOptions).toEqual(ollamaOk.embedding_models);
  });

  it('probeOllama 失败分支：状态置空并记录错误', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({
      status: 'error',
      error: 'OLLAMA_UNAVAILABLE',
    });

    await useSettingsStore.getState().probeOllama();

    const s = useSettingsStore.getState();
    expect(s.ollamaStatus).toBeNull();
    expect(s.error).toBe('OLLAMA_UNAVAILABLE');
    expect(s.ollamaProbing).toBe(false);
  });

  it('probeOllama 异常（invoke reject）也复位 probing', async () => {
    (fileIpc.ollamaStatus as Mock).mockRejectedValue(new Error('sidecar down'));

    await expect(useSettingsStore.getState().probeOllama()).rejects.toThrow('sidecar down');
    expect(useSettingsStore.getState().ollamaProbing).toBe(false);
  });

  it('probeOllama TTL 节流：60s 内已成功探测则复用，不再发 IPC', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);
  });

  it('probeOllama force=true：绕过节流强制重探', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    await useSettingsStore.getState().probeOllama(true);

    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(2);
  });

  it('probeOllama 失败不缓存：下次调用照常重探', async () => {
    (fileIpc.ollamaStatus as Mock)
      .mockResolvedValueOnce({ status: 'error', error: 'down' })
      .mockResolvedValueOnce({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    expect(useSettingsStore.getState().ollamaStatus).toBeNull();

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(2);
    expect(useSettingsStore.getState().ollamaStatus?.available).toBe(true);
  });

  it('setLlmModel：乐观更新并持久化', async () => {
    (fileIpc.updateConfig as Mock).mockResolvedValue({
      status: 'ok',
      data: { ...baseConfig, llm_model: 'qwen2.5' },
    });

    await useSettingsStore.getState().setLlmModel('qwen2.5');

    expect(useSettingsStore.getState().llmModel).toBe('qwen2.5');
    expect(fileIpc.updateConfig).toHaveBeenCalledWith(
      expect.objectContaining({ llm_model: 'qwen2.5' }),
    );
  });

  it('updateConfig 成功：合并当前状态提交并本地合并（不再重拉配置）', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });

    await useSettingsStore.getState().updateConfig({ max_file_size_mb: 200 });

    const sent = (fileIpc.updateConfig as Mock).mock.calls[0][0] as AppConfig;
    expect(sent.max_file_size_mb).toBe(200);
    expect(sent.llm_model).toBe('qwen3.8-27b'); // 未传字段由当前状态合并
    // 优化契约：成功后用发送值本地合并，不再全量 getConfig 重拉
    //（省一次串行 IPC，且避免 loadConfig 置 isLoading 闪全屏加载态）
    expect(fileIpc.getConfig).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().maxFileSizeMb).toBe(200);
    expect(useSettingsStore.getState().llmModel).toBe('qwen3.8-27b');
  });

  it('updateConfig 不得清空已签署的同意书字段（FE-B1 回归）', async () => {
    // 模拟已签同意书状态后，仅更新无关字段（如切模型）
    useSettingsStore.setState({
      cloudConsentSigned: true,
      cloudConsentVersion: CLOUD_CONSENT_VERSION,
      cloudConsentProvider: 'Openai',
      cloudConsentSignedAt: '2026-08-24T00:00:00Z',
    });
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });

    await useSettingsStore.getState().updateConfig({ llm_model: 'qwen2.5' });

    // 后端 upsert 是全字段覆盖：未传的同意书字段必须回填当前值而非 null
    const sent = (fileIpc.updateConfig as Mock).mock.calls[0][0] as AppConfig;
    expect(sent.cloud_consent_signed).toBe(true);
    expect(sent.cloud_consent_version).toBe(CLOUD_CONSENT_VERSION);
    expect(sent.cloud_consent_provider).toBe('Openai');
    expect(sent.cloud_consent_signed_at).toBe('2026-08-24T00:00:00Z');
  });

  it('updateConfig 失败：置错并抛异常', async () => {
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'error', error: 'CFG-INVALID' });

    await expect(useSettingsStore.getState().updateConfig({ language: 'en' })).rejects.toThrow(
      'CFG-INVALID',
    );
    expect(useSettingsStore.getState().error).toBe('CFG-INVALID');
  });

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

  it('completeOnboarding：提交数据目录与完成标记', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });

    await useSettingsStore.getState().completeOnboarding('/data');

    expect(fileIpc.updateConfig).toHaveBeenCalledWith(
      expect.objectContaining({ data_directory: '/data', onboarding_completed: true }),
    );
  });

  it('setTheme：持久化并应用到 html data-theme', () => {
    useSettingsStore.getState().setTheme(ThemeMode.Dark);

    expect(useSettingsStore.getState().theme).toBe(ThemeMode.Dark);
    expect(document.documentElement.getAttribute('data-theme')).toBe(ThemeMode.Dark);
  });
});

describe('settingsStore 批次10修复（FE-M5/M11/M12）', () => {
  beforeEach(resetSettings);

  it('FE-M11: loadConfig IPC 异常（Error rethrow）→ initFailed=true + isLoading=false', async () => {
    (fileIpc.getConfig as Mock).mockRejectedValue(new Error('IPC 断连'));

    await useSettingsStore.getState().loadConfig();

    const s = useSettingsStore.getState();
    expect(s.initFailed).toBe(true);
    expect(s.isLoading).toBe(false);
    expect(s.error).toBe('IPC 断连');
  });

  it('FE-M11: loadConfig 业务错误（status=error）→ initFailed=true', async () => {
    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'error', error: 'DB 锁定' });

    await useSettingsStore.getState().loadConfig();

    const s = useSettingsStore.getState();
    expect(s.initFailed).toBe(true);
    expect(s.isLoading).toBe(false);
    expect(s.error).toBe('DB 锁定');
  });

  it('FE-M11: retryInit 成功后 initFailed 复位', async () => {
    (fileIpc.getConfig as Mock).mockRejectedValueOnce(new Error('第一次失败'));
    await useSettingsStore.getState().loadConfig();
    expect(useSettingsStore.getState().initFailed).toBe(true);

    (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
    await useSettingsStore.getState().retryInit();

    const s = useSettingsStore.getState();
    expect(s.initFailed).toBe(false);
    expect(s.isLoading).toBe(false);
  });

  it('FE-M5: setLlmModel 持久化失败回滚旧值并 rethrow', async () => {
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'error', error: '写盘失败' });
    // updateConfig 失败路径会 set error + throw；setLlmModel 捕获后回滚

    await expect(useSettingsStore.getState().setLlmModel('llama3')).rejects.toThrow('写盘失败');

    expect(useSettingsStore.getState().llmModel).toBe('qwen3.8-27b');
  });

  it('FE-M12: loadApiKeyStatus 部分返回 → 其余 provider 保留默认而非 undefined', async () => {
    (fileIpc.getApiKeyStatus as Mock).mockResolvedValue({
      status: 'ok',
      data: [{ provider: 'Openai', has_key: true, hint: '***abcd' }],
    });

    await useSettingsStore.getState().loadApiKeyStatus();

    const map = useSettingsStore.getState().apiKeyStatus;
    expect(map.Openai.has_key).toBe(true);
    expect(map.Deepseek).toEqual({ provider: 'Deepseek', has_key: false, hint: '' });
  });
});
