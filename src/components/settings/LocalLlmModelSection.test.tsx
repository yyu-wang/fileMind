// LocalLlmModelSection 单元测试：GGUF 权重状态徽标、下载入口、进度、失败重试与映射归属。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import type { ModelDownloadStatus } from '@/types/ipc';
import { useSettingsStore } from '@/stores/settingsStore';

import { LOCAL_LLM_MODEL_NAME, LocalLlmModelSection } from './LocalLlmModelSection';

/** 构造下载状态（默认进行中，约 50%）。 */
function downloadStatus(overrides: Partial<ModelDownloadStatus> = {}): ModelDownloadStatus {
  return {
    model_name: LOCAL_LLM_MODEL_NAME,
    status: 'downloading',
    mirror: 'https://hf-mirror.com',
    attempt: 1,
    downloaded_bytes: 1_000_000_000,
    total_bytes: 2_000_000_000,
    error: null,
    updated_at: '2026-09-16T10:00:00',
    ...overrides,
  };
}

/** 隔离存储：动作替换为桩，避免测试触达真实 IPC；下载状态按 model_name 放进映射。 */
function seed(
  downloads: Record<string, ModelDownloadStatus> = {},
  installErrors: Record<string, string> = {},
): void {
  localStorage.clear();
  useSettingsStore.setState({
    modelDownloads: downloads,
    installErrors,
    startModelDownload: vi.fn(async () => {}),
    refreshModelDownload: vi.fn(async () => {}),
  });
}

beforeEach(() => seed());

describe('LocalLlmModelSection', () => {
  it('offers download entry when gguf weights missing', () => {
    render(<LocalLlmModelSection />);
    // 未装 Ollama 时的定位说明 + 未下载徽标
    expect(screen.getByText(/未安装 Ollama 时/)).toBeInTheDocument();
    expect(screen.getByTestId('local-llm-status')).toHaveTextContent('未下载');
    expect(screen.getByText(/需要把 GGUF 权重下载到本机（约 2 GB）/)).toBeInTheDocument();

    const button = screen.getByTestId('local-llm-download-btn');
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(useSettingsStore.getState().startModelDownload).toHaveBeenCalledWith(
      LOCAL_LLM_MODEL_NAME,
    );
  });

  it('shows progress bar with percent and disables button while downloading', () => {
    seed({ [LOCAL_LLM_MODEL_NAME]: downloadStatus({ attempt: 2 }) });
    render(<LocalLlmModelSection />);

    expect(screen.getByTestId('local-llm-download-progress')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByText(/正在下载本地生成模型：50%/)).toBeInTheDocument();
    expect(screen.getByText(/第 2 次尝试/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '下载中...' })).toBeDisabled();
  });

  it('shows indeterminate progress text when total size unknown', () => {
    seed({ [LOCAL_LLM_MODEL_NAME]: downloadStatus({ total_bytes: null }) });
    render(<LocalLlmModelSection />);
    expect(screen.getByText(/已下载 953.7 MB/)).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
  });

  it('shows failure reason and retry button after auto retries exhausted', () => {
    seed({
      [LOCAL_LLM_MODEL_NAME]: downloadStatus({
        status: 'failed',
        attempt: 3,
        downloaded_bytes: 0,
        error: '下载失败（已自动重试 3 次，切换镜像均未成功）',
      }),
    });
    render(<LocalLlmModelSection />);
    expect(screen.getByTestId('local-llm-download-failure')).toHaveTextContent('已自动重试 3 次');
    expect(screen.getByRole('button', { name: '重新下载' })).toBeEnabled();
  });

  it('shows ready badge and no download button when weights present', () => {
    seed({ [LOCAL_LLM_MODEL_NAME]: downloadStatus({ status: 'ready' }) });
    render(<LocalLlmModelSection />);
    expect(screen.getByTestId('local-llm-status')).toHaveTextContent('已就绪');
    expect(screen.queryByTestId('local-llm-download-btn')).not.toBeInTheDocument();
    expect(screen.queryByTestId('local-llm-download-progress')).not.toBeInTheDocument();
  });

  it('ignore download status belonging to another model', () => {
    // 映射里只有 Embedding 模型的条目：本卡片读不到自己那份，应显示未下载
    seed({ 'bge-large-zh-v1.5': downloadStatus({ model_name: 'bge-large-zh-v1.5' }) });
    render(<LocalLlmModelSection />);
    expect(screen.getByTestId('local-llm-status')).toHaveTextContent('未下载');
    expect(screen.queryByTestId('local-llm-download-progress')).not.toBeInTheDocument();
  });

  it('shows start-request failure for this model', () => {
    // 启动下载这一步失败（如 Sidecar 未就绪）必须可见：否则点了按钮毫无反馈
    seed({}, { [LOCAL_LLM_MODEL_NAME]: 'SIDE-U-001:AI 服务启动失败' });
    render(<LocalLlmModelSection />);

    expect(screen.getByTestId('local-llm-install-failure')).toHaveTextContent('SIDE-U-001');
  });

  it('ignore start-request failure belonging to another model', () => {
    seed({}, { 'bge-large-zh-v1.5': 'SIDE-U-001:AI 服务启动失败' });
    render(<LocalLlmModelSection />);

    expect(screen.queryByTestId('local-llm-install-failure')).not.toBeInTheDocument();
  });
});
