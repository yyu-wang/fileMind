// SettingsPage 单元测试：挂载触发 Ollama 探测、各设置区块渲染与云端同意信息展示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import type {
  ApiKeyStatus,
  AppConfig,
  CloudProviderRecord,
  FileStats,
  OllamaStatus,
} from '@/types/ipc';
import { ThemeMode } from '@/types/models';

import { useFileStore } from '@/stores/fileStore';
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
  getFileStats: vi.fn(),
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
    getFileStats: mocks.getFileStats,
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

// P-07：CloudApiKeySection 的 Key 行来自 store.cloudProviders，seed 两条内置记录
//（name 需命中 `OpenAI API Key 输入框` 等 DOM 查询；不 mock listCloudProviders，
//  否则 CloudProviderManager 会连带再触发一次 loadApiKeyStatus，破坏调用次数断言）
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

const emptyStats: FileStats = {
  total_files: 0,
  categorized_files: 0,
  uncategorized_files: 0,
  duplicate_groups: 0,
  total_size_bytes: 0,
};

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
    cloudProviders: builtinCloudProviders,
    cloudProvidersLoading: false,
  });
  // 重置 fileStore，避免 EmbeddingModelSection useEffect 触发真实 loadStats
  useFileStore.setState({ stats: emptyStats, total: 0, files: [], selectedIds: [] });
  mocks.ollamaStatus.mockResolvedValue({ status: 'ok', data: ollamaOk });
  mocks.getConfig.mockResolvedValue({ status: 'ok', data: baseConfig });
  mocks.getApiKeyStatus.mockResolvedValue({ status: 'ok', data: [noKey, noDeep] });
  mocks.getFileStats.mockResolvedValue({ status: 'ok', data: emptyStats });
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
    expect(await screen.findByRole('heading', { level: 3, name: /推理模式/ })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 3, name: /AI 模型配置/ })).toBeInTheDocument();
    expect(screen.getByText('跟随系统')).toBeInTheDocument();
  });

  it('shows local mode-option selected by default', () => {
    renderPage();
    const localOption = screen.getByTestId('mode-option-local');
    expect(localOption).toHaveAttribute('aria-checked', 'true');
    expect(localOption).toHaveAccessibleName(/本地模式（当前）/);
  });

  it('loads api key status on mount and shows 未配置', async () => {
    renderPage();
    expect(mocks.getApiKeyStatus).toHaveBeenCalledTimes(1);
    expect(await screen.findAllByText('未配置')).toHaveLength(2);
  });
});
