// settingsStore 单元测试：配置加载/推理模式切换/Ollama 探测/配置更新/主题/初始化失败回滚。
//
// 拆分说明（原单文件 572 行超测试文件警告阈值 400，且主 describe 达 311 行触函数行数基线）：
//   - 共享夹具见 ./settingsTestFixtures.ts
//   - 云端同意书与 API Key 见 ./settingsStore.cloud.test.ts
//   - 离线模型包导入 / 本地生成后端配置见 ./settingsStore.models.test.ts

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
import { ThemeMode } from '../types/models';
import type { AppConfig } from '../types/ipc';
import { useSettingsStore } from './settingsStore';
import { baseConfig, ollamaOk, resetSettings } from './settingsTestFixtures';

describe('settingsStore · 配置加载与推理模式', () => {
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

describe('settingsStore · 配置更新', () => {
  beforeEach(resetSettings);

  it('成功：合并当前状态提交并本地合并（不再重拉配置）', async () => {
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

  it('不得清空已签署的同意书字段（FE-B1 回归）', async () => {
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

  it('失败：置错并抛异常', async () => {
    (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'error', error: 'CFG-INVALID' });

    await expect(useSettingsStore.getState().updateConfig({ language: 'en' })).rejects.toThrow(
      'CFG-INVALID',
    );
    expect(useSettingsStore.getState().error).toBe('CFG-INVALID');
  });
});

describe('settingsStore · Ollama 探测', () => {
  beforeEach(resetSettings);

  it('成功：写入状态与模型选项', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();

    const s = useSettingsStore.getState();
    expect(s.ollamaProbing).toBe(false);
    expect(s.ollamaStatus).toEqual(ollamaOk);
    expect(s.llmModelOptions).toEqual(ollamaOk.llm_models);
    expect(s.embeddingModelOptions).toEqual(ollamaOk.embedding_models);
  });

  it('失败分支：状态置空并记录错误', async () => {
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

  it('异常（invoke reject）也复位 probing', async () => {
    (fileIpc.ollamaStatus as Mock).mockRejectedValue(new Error('sidecar down'));

    await expect(useSettingsStore.getState().probeOllama()).rejects.toThrow('sidecar down');
    expect(useSettingsStore.getState().ollamaProbing).toBe(false);
  });

  it('TTL 节流：60s 内已成功探测则复用，不再发 IPC', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);
  });

  it('force=true：绕过节流强制重探', async () => {
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    await useSettingsStore.getState().probeOllama(true);

    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(2);
  });

  it('失败不缓存：下次调用照常重探', async () => {
    (fileIpc.ollamaStatus as Mock)
      .mockResolvedValueOnce({ status: 'error', error: 'down' })
      .mockResolvedValueOnce({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().probeOllama();
    expect(useSettingsStore.getState().ollamaStatus).toBeNull();

    await useSettingsStore.getState().probeOllama();
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(2);
    expect(useSettingsStore.getState().ollamaStatus?.available).toBe(true);
  });
});

describe('settingsStore · 初始化失败与回滚（FE-M5/M11）', () => {
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
});
