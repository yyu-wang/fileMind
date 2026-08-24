// SettingsPage 单元测试：挂载触发 Ollama 探测、各设置区块渲染与云端同意信息展示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { ApiKeyStatus, AppConfig, OllamaStatus } from '@/types/ipc';
import { ThemeMode } from '@/types/models';

import { useSettingsStore } from '@/stores/settingsStore';
import { SettingsPage } from './SettingsPage';

const mocks = vi.hoisted(() => ({
  ollamaStatus: vi.fn(),
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
  getApiKeyStatus: vi.fn(),
  setApiKey: vi.fn(),
  deleteApiKey: vi.fn(),
  signCloudConsent: vi.fn(),
  revokeCloudConsent: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    ollamaStatus: mocks.ollamaStatus,
    getConfig: mocks.getConfig,
    updateConfig: mocks.updateConfig,
    getApiKeyStatus: mocks.getApiKeyStatus,
    setApiKey: mocks.setApiKey,
    deleteApiKey: mocks.deleteApiKey,
    signCloudConsent: mocks.signCloudConsent,
    revokeCloudConsent: mocks.revokeCloudConsent,
  },
}));

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

const noKey: ApiKeyStatus = { provider: 'Openai', has_key: false, hint: '' };
const noDeep: ApiKeyStatus = { provider: 'Deepseek', has_key: false, hint: '' };

beforeEach(() => {
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
    isLoading: false,
    error: null,
    ollamaStatus: null,
    ollamaProbing: false,
    llmModelOptions: [],
    embeddingModelOptions: [],
    theme: ThemeMode.System,
    apiKeyStatus: { Openai: noKey, Deepseek: noDeep },
  });
  mocks.ollamaStatus.mockResolvedValue({ status: 'ok', data: ollamaOk });
  mocks.getConfig.mockResolvedValue({ status: 'ok', data: baseConfig });
  mocks.getApiKeyStatus.mockResolvedValue({ status: 'ok', data: [noKey, noDeep] });
});

function renderPage(): void {
  render(<SettingsPage />);
}

describe('SettingsPage', () => {
  it('renders title and probes Ollama on mount', async () => {
    renderPage();
    expect(screen.getByText('设置')).toBeInTheDocument();
    expect(mocks.ollamaStatus).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/Ollama 可用/)).toBeInTheDocument();
  });

  it('renders all setting sections', async () => {
    renderPage();
    expect(await screen.findByText('推理模式')).toBeInTheDocument();
    expect(screen.getByText('云端 API Key')).toBeInTheDocument();
    expect(screen.getByText('跟随系统')).toBeInTheDocument();
  });

  it('shows local mode badge by default', () => {
    renderPage();
    expect(screen.getByText('🛡️ 本地模式')).toBeInTheDocument();
  });

  it('loads api key status on mount and shows 未配置', async () => {
    renderPage();
    expect(mocks.getApiKeyStatus).toHaveBeenCalledTimes(1);
    expect(await screen.findAllByText('未配置')).toHaveLength(2);
  });
});
