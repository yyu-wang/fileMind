// OllamaStatusCard 单元测试：状态徽标、重新检测、LLM 下拉。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('@/lib/ipc', () => ({
  fileIpc: {
    ollamaStatus: vi.fn(),
    updateConfig: vi.fn(),
    getConfig: vi.fn(),
  },
}));

import { fileIpc } from '@/lib/ipc';
import { ThemeMode } from '@/types/models';
import type { AppConfig, OllamaStatus } from '@/types/ipc';
import { useSettingsStore } from '@/stores/settingsStore';

import { OllamaStatusCard } from './OllamaStatusCard';

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

const ollamaDown: OllamaStatus = {
  ...ollamaOk,
  available: false,
  status: 'unavailable',
  llm_models: [],
  embedding_models: [],
  error_code: 'OLLAMA_UNAVAILABLE',
  message: 'Ollama 服务未启动',
};

function seed(overrides: Partial<Parameters<typeof useSettingsStore.setState>[0]> = {}): void {
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
    ...overrides,
  });
  (fileIpc.updateConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
  (fileIpc.getConfig as Mock).mockResolvedValue({ status: 'ok', data: baseConfig });
}

beforeEach(() => seed());

describe('OllamaStatusCard', () => {
  it('shows available badge and enabled llm select when ollama ok', () => {
    seed({
      ollamaStatus: ollamaOk,
      llmModelOptions: ollamaOk.llm_models,
      embeddingModelOptions: ollamaOk.embedding_models,
    });
    render(<OllamaStatusCard />);
    expect(screen.getByRole('status')).toHaveTextContent('● Ollama 可用');
    const select = screen.getByLabelText('本地 LLM 模型');
    expect(select).not.toBeDisabled();
    expect(screen.getByText('qwen3.8-27b')).toBeInTheDocument();
  });

  it('shows unavailable badge with detail and disabled select', () => {
    seed({ ollamaStatus: ollamaDown });
    render(<OllamaStatusCard />);
    expect(screen.getByRole('status')).toHaveTextContent('○ Ollama 不可用');
    expect(screen.getByText('Ollama 服务未启动')).toBeInTheDocument();
    expect(screen.getByLabelText('本地 LLM 模型')).toBeDisabled();
  });

  it('shows probing state and disables re-detect button', () => {
    seed({ ollamaProbing: true });
    render(<OllamaStatusCard />);
    expect(screen.getByRole('status')).toHaveTextContent('正在检测 Ollama...');
    expect(screen.getByRole('button', { name: '重新检测' })).toBeDisabled();
  });

  it('re-detect button triggers probeOllama', async () => {
    const user = userEvent.setup();
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });
    render(<OllamaStatusCard />);
    await user.click(screen.getByRole('button', { name: '重新检测' }));
    expect(fileIpc.ollamaStatus).toHaveBeenCalled();
  });

  it('selecting a different llm model persists via updateConfig', async () => {
    const user = userEvent.setup();
    const models = [
      { name: 'qwen3.8-27b', size_bytes: 10, family: 'qwen', modified_at: null as string | null },
      { name: 'llama3.1', size_bytes: 20, family: 'llama', modified_at: null },
    ];
    seed({
      ollamaStatus: ollamaOk,
      llmModelOptions: models,
      embeddingModelOptions: ollamaOk.embedding_models,
    });
    render(<OllamaStatusCard />);
    await user.selectOptions(screen.getByLabelText('本地 LLM 模型'), 'llama3.1');
    expect(fileIpc.updateConfig).toHaveBeenCalledWith(
      expect.objectContaining({ llm_model: 'llama3.1' }),
    );
  });
});
