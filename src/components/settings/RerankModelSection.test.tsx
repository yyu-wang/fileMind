// RerankModelSection 单元测试：状态徽标、下载入口、进度、失败重试与状态槽位归属。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import type { ModelDownloadStatus } from '@/types/ipc';
import { useSettingsStore } from '@/stores/settingsStore';

import { RERANK_MODEL_NAME, RerankModelSection } from './RerankModelSection';

/** 构造下载状态（默认进行中，约 50%）。 */
function downloadStatus(overrides: Partial<ModelDownloadStatus> = {}): ModelDownloadStatus {
  return {
    model_name: RERANK_MODEL_NAME,
    status: 'downloading',
    mirror: 'https://hf-mirror.com',
    attempt: 1,
    downloaded_bytes: 1_150_000_000,
    total_bytes: 2_300_000_000,
    error: null,
    updated_at: '2026-09-16T10:00:00',
    ...overrides,
  };
}

/** 隔离存储：动作替换为桩，避免测试触达真实 IPC。 */
function seed(modelDownload: ModelDownloadStatus | null = null): void {
  localStorage.clear();
  useSettingsStore.setState({
    modelDownload,
    installError: null,
    startModelDownload: vi.fn(async () => {}),
    refreshModelDownload: vi.fn(async () => {}),
  });
}

beforeEach(() => seed());

describe('RerankModelSection', () => {
  it('offers download entry when rerank model missing', () => {
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('未下载');
    expect(screen.getByText(new RegExp(RERANK_MODEL_NAME))).toBeInTheDocument();
    // 未就绪不中断问答，但要说明相关性会下降
    expect(screen.getByText(/问答仍可用，但结果相关性会下降/)).toBeInTheDocument();

    const button = screen.getByTestId('rerank-download-btn');
    fireEvent.click(button);
    expect(useSettingsStore.getState().startModelDownload).toHaveBeenCalledWith(RERANK_MODEL_NAME);
  });

  it('shows ready badge and no download button when files present', () => {
    seed(downloadStatus({ status: 'ready' }));
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('已就绪');
    expect(screen.queryByTestId('rerank-download-btn')).not.toBeInTheDocument();
  });

  it('ignore download status belonging to another model', () => {
    // 共享状态槽位：Embedding 的下载状态不应影响本区块的展示
    seed(downloadStatus({ model_name: 'bge-large-zh-v1.5' }));
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('未下载');
    expect(screen.queryByTestId('rerank-download-progress')).not.toBeInTheDocument();
  });

  it('shows progress bar with percent, mirror while downloading', () => {
    seed(downloadStatus({ attempt: 2 }));
    render(<RerankModelSection />);

    expect(screen.getByTestId('rerank-download-progress')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByText(/正在下载重排模型：50%/)).toBeInTheDocument();
    expect(screen.getByText(/第 2 次尝试/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '下载中...' })).toBeDisabled();
  });

  it('shows failure reason and retry button after auto retries exhausted', () => {
    seed(
      downloadStatus({
        status: 'failed',
        attempt: 3,
        downloaded_bytes: 0,
        error: '下载失败（已自动重试 3 次，切换镜像均未成功）',
      }),
    );
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-download-failure')).toHaveTextContent('已自动重试 3 次');
    expect(screen.getByRole('button', { name: '重新下载' })).toBeEnabled();
  });
});
