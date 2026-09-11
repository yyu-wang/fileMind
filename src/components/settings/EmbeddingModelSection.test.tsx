// EmbeddingModelSection 单元测试：当前模型 panel、可用性列表与空态展示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import type { FileStats } from '@/types/ipc';
import { ThemeMode } from '@/types/models';
import { useFileStore } from '@/stores/fileStore';
import { useSettingsStore } from '@/stores/settingsStore';

import { EmbeddingModelSection } from './EmbeddingModelSection';

const emptyStats: FileStats = {
  total_files: 0,
  categorized_files: 0,
  uncategorized_files: 0,
  duplicate_groups: 0,
  total_size_bytes: 0,
};

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
    temperature: 0.2,
    installingModel: null,
    installError: null,
    apiKeyStatus: {
      Openai: { provider: 'Openai', has_key: false, hint: '' },
      Deepseek: { provider: 'Deepseek', has_key: false, hint: '' },
    },
    cloudModel: '',
    ...overrides,
  });
  // 重置 fileStore，避免 useEffect 触发真实 loadStats
  useFileStore.setState({ stats: emptyStats, total: 0, files: [], selectedIds: [] });
}

beforeEach(() => seed());

describe('EmbeddingModelSection', () => {
  it('renders empty list hint when no models probed', () => {
    render(<EmbeddingModelSection />);
    expect(screen.getByText('暂无模型列表')).toBeInTheDocument();
    expect(screen.getByText('请先检测 Ollama 环境')).toBeInTheDocument();
    // panel 始终展示「已锁定」徽标
    expect(screen.getByText('已锁定')).toBeInTheDocument();
  });

  it('renders current model panel with dim/version and list installed badge', () => {
    seed({
      embeddingModel: 'bge-small-zh',
      embeddingModelOptions: [{ name: 'bge-small-zh', dim: 512, version: 1, available: true }],
    });
    render(<EmbeddingModelSection />);
    expect(screen.getByText(/bge-small-zh · dim 512 · v1/)).toBeInTheDocument();
    // panel 的「已锁定」徽标
    expect(screen.getByText('已锁定')).toBeInTheDocument();
    // 列表行一个「已安装」徽标（panel 不再展示「已安装」）
    expect(screen.getAllByText('已安装')).toHaveLength(1);
  });

  it('shows missing badge for uninstalled current model in list', () => {
    seed({
      embeddingModel: 'bge-large-zh-v1.5',
      embeddingModelOptions: [
        { name: 'bge-large-zh-v1.5', dim: 1024, version: 1, available: false },
      ],
    });
    render(<EmbeddingModelSection />);
    // 列表行一个「未安装」徽标
    expect(screen.getAllByText('未安装')).toHaveLength(1);
  });

  it('renders install button for uninstalled model and hides switch buttons', () => {
    seed({
      embeddingModel: 'bge-small-zh',
      embeddingModelOptions: [
        { name: 'bge-small-zh', dim: 512, version: 1, available: true },
        { name: 'bge-m3', dim: 1024, version: 1, available: false },
      ],
    });
    render(<EmbeddingModelSection />);
    // 未安装模型按钮文案「安装」，Ollama 不可用时 disabled
    expect(screen.getByRole('button', { name: '安装' })).toBeDisabled();
    // P1 未开发的模型切换功能不渲染占位按钮
    expect(screen.queryByRole('button', { name: '切换' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '当前' })).not.toBeInTheDocument();
  });

  it('renders indexed file count from fileStore stats', () => {
    seed({
      embeddingModel: 'bge-small-zh',
      embeddingModelOptions: [{ name: 'bge-small-zh', dim: 512, version: 1, available: true }],
    });
    useFileStore.setState({
      stats: {
        total_files: 100,
        categorized_files: 42,
        uncategorized_files: 58,
        duplicate_groups: 0,
        total_size_bytes: 0,
      },
    });
    render(<EmbeddingModelSection />);
    expect(screen.getByText(/已索引 42 文件/)).toBeInTheDocument();
  });
});
