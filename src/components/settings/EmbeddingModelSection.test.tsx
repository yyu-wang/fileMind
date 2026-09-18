// EmbeddingModelSection 单元测试：当前模型 panel、模型就绪状态、下载进度与空态展示。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import type { FileStats, ModelDownloadStatus } from '@/types/ipc';
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

/** 构造下载状态（默认进行中，约 50%）。 */
function downloadStatus(overrides: Partial<ModelDownloadStatus> = {}): ModelDownloadStatus {
  return {
    model_name: 'bge-large-zh-v1.5',
    status: 'downloading',
    mirror: 'https://hf-mirror.com',
    attempt: 1,
    downloaded_bytes: 163_840_000,
    total_bytes: 327_680_000,
    error: null,
    updated_at: '2026-09-15T10:00:00',
    ...overrides,
  };
}

function seed(overrides: Partial<Parameters<typeof useSettingsStore.setState>[0]> = {}): void {
  localStorage.clear();
  useSettingsStore.setState({
    inferenceMode: 'Local',
    llmModel: 'qwen3.8-27b',
    dataDirectory: '',
    embeddingModel: 'bge-large-zh-v1.5',
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
    downloadingModel: null,
    modelDownloads: {},
    installErrors: {},
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

const READY_MODEL = {
  name: 'bge-large-zh-v1.5',
  dim: 1024,
  version: 1,
  available: true,
} as const;

const MISSING_MODEL = {
  name: 'bge-large-zh-v1.5',
  dim: 1024,
  version: 1,
  available: false,
} as const;

describe('EmbeddingModelSection', () => {
  it('renders empty list hint when no models probed', () => {
    render(<EmbeddingModelSection />);
    expect(screen.getByText('暂无模型列表')).toBeInTheDocument();
    expect(screen.getByText('请先在设置页完成本地环境检测')).toBeInTheDocument();
    // panel 始终展示「已锁定」徽标
    expect(screen.getByText('已锁定')).toBeInTheDocument();
  });

  it('renders current model panel with dim/version and ready badge', () => {
    seed({ embeddingModelOptions: [READY_MODEL] });
    render(<EmbeddingModelSection />);
    expect(screen.getByText(/bge-large-zh-v1.5 · dim 1024 · v1/)).toBeInTheDocument();
    expect(screen.getByText('已锁定')).toBeInTheDocument();
    // 列表行一个「已就绪」徽标 + panel 内的就绪说明
    expect(screen.getAllByText('已就绪')).toHaveLength(1);
    expect(screen.getByTestId('model-ready')).toBeInTheDocument();
    // 就绪后不再提供下载按钮
    expect(screen.queryByRole('button', { name: '下载' })).not.toBeInTheDocument();
  });

  it('offers download button and guidance when model files missing', () => {
    seed({ embeddingModelOptions: [MISSING_MODEL] });
    render(<EmbeddingModelSection />);
    // 未下载徽标 + 说明文案
    expect(screen.getAllByText('未下载')).toHaveLength(1);
    expect(screen.getByText(/需下载到本机后才能使用/)).toBeInTheDocument();
    // 下载按钮可用（不再依赖 Ollama 环境）
    const button = screen.getByRole('button', { name: '下载' });
    expect(button).toBeEnabled();
    // P1 未开发的模型切换功能不渲染占位按钮
    expect(screen.queryByRole('button', { name: '切换' })).not.toBeInTheDocument();
  });

  it('shows progress bar with percent, mirror and attempt while downloading', () => {
    seed({
      embeddingModelOptions: [MISSING_MODEL],
      downloadingModel: 'bge-large-zh-v1.5',
      modelDownloads: { 'bge-large-zh-v1.5': downloadStatus({ attempt: 2 }) },
    });
    render(<EmbeddingModelSection />);

    expect(screen.getByTestId('model-download-progress')).toBeInTheDocument();
    const bar = screen.getByRole('progressbar');
    expect(bar).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByText(/正在下载模型文件：50%/)).toBeInTheDocument();
    expect(screen.getByText(/第 2 次尝试/)).toBeInTheDocument();
    expect(screen.getByText(/镜像 https:\/\/hf-mirror.com/)).toBeInTheDocument();
    // 下载中按钮禁用且文案切换
    expect(screen.getByRole('button', { name: '下载中...' })).toBeDisabled();
  });

  it('shows indeterminate progress text when total size unknown', () => {
    seed({
      embeddingModelOptions: [MISSING_MODEL],
      downloadingModel: 'bge-large-zh-v1.5',
      modelDownloads: { 'bge-large-zh-v1.5': downloadStatus({ total_bytes: null }) },
    });
    render(<EmbeddingModelSection />);
    expect(screen.getByText(/已下载 156.3 MB/)).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
  });

  it('shows failure reason and retry button after auto retries exhausted', () => {
    seed({
      embeddingModelOptions: [MISSING_MODEL],
      modelDownloads: {
        'bge-large-zh-v1.5': downloadStatus({
          status: 'failed',
          attempt: 3,
          downloaded_bytes: 0,
          error: '下载失败（已自动重试 3 次，切换镜像均未成功）',
        }),
      },
    });
    render(<EmbeddingModelSection />);
    expect(screen.getByRole('alert')).toHaveTextContent('下载失败');
    expect(screen.getByRole('alert')).toHaveTextContent('已自动重试 3 次');
    expect(screen.getByRole('button', { name: '重新下载' })).toBeEnabled();
  });

  it('renders indexed file count from fileStore stats', () => {
    seed({ embeddingModelOptions: [READY_MODEL] });
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
