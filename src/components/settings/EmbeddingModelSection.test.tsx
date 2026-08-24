// EmbeddingModelSection 单元测试：当前模型徽标、可用性列表与空态。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import { ThemeMode } from '@/types/models';
import { useSettingsStore } from '@/stores/settingsStore';

import { EmbeddingModelSection } from './EmbeddingModelSection';

function seed(overrides: Partial<Parameters<typeof useSettingsStore.setState>[0]> = {}): void {
  localStorage.clear();
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
}

beforeEach(() => seed());

describe('EmbeddingModelSection', () => {
  it('renders empty list hint when no models probed', () => {
    render(<EmbeddingModelSection />);
    expect(screen.getByText('暂无模型列表')).toBeInTheDocument();
    expect(screen.getByText('请先检测 Ollama 环境')).toBeInTheDocument();
    expect(screen.getByText('未知')).toBeInTheDocument();
  });

  it('renders current model badge with dim/version and installed badge', () => {
    seed({
      embeddingModel: 'bge-small-zh',
      embeddingModelOptions: [{ name: 'bge-small-zh', dim: 512, version: 1, available: true }],
    });
    render(<EmbeddingModelSection />);
    expect(screen.getByText(/bge-small-zh · dim 512 · v1/)).toBeInTheDocument();
    // 当前行 + 列表行各一个已安装徽标
    expect(screen.getAllByText('已安装')).toHaveLength(2);
  });

  it('shows missing badge for uninstalled current model', () => {
    seed({
      embeddingModel: 'bge-large-zh-v1.5',
      embeddingModelOptions: [
        { name: 'bge-large-zh-v1.5', dim: 1024, version: 1, available: false },
      ],
    });
    render(<EmbeddingModelSection />);
    // 当前行 + 列表行各一个未安装徽标
    expect(screen.getAllByText('未安装')).toHaveLength(2);
  });

  it('shows 检测中 while probing', () => {
    seed({ ollamaProbing: true });
    render(<EmbeddingModelSection />);
    expect(screen.getByText('检测中')).toBeInTheDocument();
  });
});
