// RerankModelSection 单元测试：状态徽标、下载入口、进度、失败重试与下载状态映射归属。

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

vi.mock('@/lib/ipc', () => ({ fileIpc: {} }));

import type { ModelDownloadStatus } from '@/types/ipc';
import { useSettingsStore } from '@/stores/settingsStore';

import { RERANK_MODEL_NAME, RerankModelSection } from './RerankModelSection';

/** 本地 GGUF 模型名（模拟同页另一张卡片，用于验证映射互不干扰）。 */
const OTHER_MODEL_NAME = 'qwen2.5-3b-instruct';

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

/** 隔离存储：动作替换为桩，避免测试触达真实 IPC；下载状态按 model_name 放进映射。 */
function seed(downloads: Record<string, ModelDownloadStatus> = {}): void {
  localStorage.clear();
  useSettingsStore.setState({
    modelDownloads: downloads,
    installErrors: {},
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
    seed({ [RERANK_MODEL_NAME]: downloadStatus({ status: 'ready' }) });
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('已就绪');
    expect(screen.queryByTestId('rerank-download-btn')).not.toBeInTheDocument();
  });

  it('ignore download status belonging to another model', () => {
    // 映射里只有 Embedding 模型的条目：本区块读不到自己那份，应显示未下载
    seed({ 'bge-large-zh-v1.5': downloadStatus({ model_name: 'bge-large-zh-v1.5' }) });
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('未下载');
    expect(screen.queryByTestId('rerank-download-progress')).not.toBeInTheDocument();
  });

  it('keeps rerank ready while another model is downloading（单槽缺陷回归）', () => {
    // 缺陷背景：下载状态曾是全页共用的单个槽位，下载 GGUF 会顶掉 Rerank 的那份 →
    // 明明已就绪的 Rerank 卡片会显示「未下载」。改为按 model_name 的映射后，
    // 同页各卡片各读自己那一份，互不覆盖。
    seed({
      [RERANK_MODEL_NAME]: downloadStatus({ status: 'ready' }),
      [OTHER_MODEL_NAME]: downloadStatus({ model_name: OTHER_MODEL_NAME }),
    });
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-status')).toHaveTextContent('已就绪');
    // 别的模型在下载：不显示本卡片的进度，也不提供下载入口
    expect(screen.queryByTestId('rerank-download-progress')).not.toBeInTheDocument();
    expect(screen.queryByTestId('rerank-download-btn')).not.toBeInTheDocument();
  });

  it('shows progress bar with percent, mirror while downloading', () => {
    seed({ [RERANK_MODEL_NAME]: downloadStatus({ attempt: 2 }) });
    render(<RerankModelSection />);

    expect(screen.getByTestId('rerank-download-progress')).toBeInTheDocument();
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByText(/正在下载重排模型：50%/)).toBeInTheDocument();
    expect(screen.getByText(/第 2 次尝试/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '下载中...' })).toBeDisabled();
  });

  it('shows failure reason and retry button after auto retries exhausted', () => {
    seed({
      [RERANK_MODEL_NAME]: downloadStatus({
        status: 'failed',
        attempt: 3,
        downloaded_bytes: 0,
        error: '下载失败（已自动重试 3 次，切换镜像均未成功）',
      }),
    });
    render(<RerankModelSection />);
    expect(screen.getByTestId('rerank-download-failure')).toHaveTextContent('已自动重试 3 次');
    expect(screen.getByRole('button', { name: '重新下载' })).toBeEnabled();
  });
});
