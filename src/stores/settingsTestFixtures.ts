// settingsStore 系列测试的共享夹具（原内联在 settingsStore.test.ts，拆文件时抽出避免三份手抄）。
//
// 只放测试数据与状态复位：各测试文件的 vi.mock('../lib/ipc') 无法共享（vi.mock 是
// 文件级提升），仍在各自文件里声明。

import { vi } from 'vitest';

import type { AppConfig, CloudProviderRecord, OllamaStatus } from '../types/ipc';
import { ThemeMode } from '../types/models';
import { useSettingsStore } from './settingsStore';

// P-07：loadApiKeyStatus 以 store.cloudProviders 为默认项基准，seed 两条内置记录
export const builtinCloudProviders: CloudProviderRecord[] = [
  {
    id: 'builtin-openai',
    provider_key: 'Openai',
    name: 'OpenAI',
    remark: '',
    website: null,
    base_url: 'https://api.openai.com/v1',
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
    base_url: 'https://api.deepseek.com/v1',
    is_builtin: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

export const baseConfig: AppConfig = {
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

export const ollamaOk: OllamaStatus = {
  available: true,
  status: 'ok',
  llm_models: [{ name: 'qwen3.8-27b', size_bytes: 10, family: 'qwen', modified_at: null }],
  embedding_models: [{ name: 'bge-small-zh', dim: 512, version: 1, available: true }],
  error_code: null,
  message: null,
};

export function resetSettings(): void {
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
    cloudProviders: builtinCloudProviders,
    cloudProvidersLoading: false,
    modelDownloads: {},
  });
}
