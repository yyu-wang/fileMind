// 模型下载状态映射（T2b）单元测试：状态按 model_name 分槽存放，多张模型卡片互不覆盖。
//
// 独立成文件而不是并入 settingsStore.test.ts：后者已接近测试文件行数上限（400 警告 /
// 600 强制），本主题与「配置 / 同意书 / API Key」也无关（同类先例：classifyStore.exec.test.ts）。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc', () => ({
  fileIpc: {
    startModelDownload: vi.fn(),
    modelDownloadStatus: vi.fn(),
    ollamaStatus: vi.fn(),
  },
}));

import { fileIpc } from '../lib/ipc';
import type { ModelDownloadStatus, OllamaStatus } from '../types/ipc';
import { useSettingsStore } from './settingsStore';

const ollamaOk: OllamaStatus = {
  available: true,
  status: 'ok',
  llm_models: [],
  embedding_models: [],
  error_code: null,
  message: null,
};

/** 构造下载状态。 */
function download(modelName: string, status: string): ModelDownloadStatus {
  return {
    model_name: modelName,
    status,
    mirror: null,
    attempt: 1,
    downloaded_bytes: 0,
    total_bytes: 100,
    error: null,
    updated_at: '2026-09-16T10:00:00',
  };
}

/** 隔离存储：只复位本主题涉及的瞬态字段。 */
function resetSettings(): void {
  localStorage.clear();
  vi.clearAllMocks();
  useSettingsStore.setState({
    modelDownloads: {},
    downloadingModel: null,
    installErrors: {},
    error: null,
    ollamaStatus: null,
    ollamaProbing: false,
    lastOllamaProbeAt: 0,
  });
}

const RERANK = 'bge-reranker-v2-m3';
const GGUF = 'qwen2.5-3b-instruct';

describe('settingsStore · 模型下载状态映射', () => {
  beforeEach(resetSettings);

  it('startModelDownload 成功：只写该模型的条目，其它模型条目保留', async () => {
    useSettingsStore.setState({ modelDownloads: { [RERANK]: download(RERANK, 'ready') } });
    (fileIpc.startModelDownload as Mock).mockResolvedValue({
      status: 'ok',
      data: download(GGUF, 'downloading'),
    });

    await useSettingsStore.getState().startModelDownload(GGUF);

    const downloads = useSettingsStore.getState().modelDownloads;
    expect(downloads[GGUF]?.status).toBe('downloading');
    // 回归点：下载 GGUF 不应顶掉 Rerank 的那一份（单槽时代会）
    expect(downloads[RERANK]?.status).toBe('ready');
    expect(useSettingsStore.getState().downloadingModel).toBe(GGUF);
  });

  it('startModelDownload 失败：清掉该模型条目并置错（不残留上一轮状态）', async () => {
    useSettingsStore.setState({ modelDownloads: { [GGUF]: download(GGUF, 'downloading') } });
    (fileIpc.startModelDownload as Mock).mockResolvedValue({
      status: 'error',
      error: 'MODEL-NOT-FOUND',
    });

    await useSettingsStore.getState().startModelDownload(GGUF);

    const s = useSettingsStore.getState();
    expect(s.installErrors[GGUF]).toBe('MODEL-NOT-FOUND');
    expect(s.downloadingModel).toBeNull();
    // 清掉条目而不是留着下载中：卡片据此回到「未下载 + 重试」
    expect(s.modelDownloads[GGUF]).toBeUndefined();
  });

  it('refreshModelDownload：本模型首次转为 ready → 绕过节流刷新探测', async () => {
    useSettingsStore.setState({ modelDownloads: { [GGUF]: download(GGUF, 'downloading') } });
    (fileIpc.modelDownloadStatus as Mock).mockResolvedValue({
      status: 'ok',
      data: download(GGUF, 'ready'),
    });
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().refreshModelDownload(GGUF);

    expect(useSettingsStore.getState().modelDownloads[GGUF]?.status).toBe('ready');
    // 就绪状态变化要反映到可用性徽标，故强制重探（TTL 被绕过）
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);
  });

  it('refreshModelDownload：其它模型已 ready 不干扰本模型的「首次就绪」判定', async () => {
    // 单槽时代的缺陷：本模型的「上次是否已就绪」会读到别人的 ready，
    // 导致真正首次就绪时不触发探测，可用性徽标停留在旧值
    useSettingsStore.setState({ modelDownloads: { [RERANK]: download(RERANK, 'ready') } });
    (fileIpc.modelDownloadStatus as Mock).mockResolvedValue({
      status: 'ok',
      data: download(GGUF, 'ready'),
    });
    (fileIpc.ollamaStatus as Mock).mockResolvedValue({ status: 'ok', data: ollamaOk });

    await useSettingsStore.getState().refreshModelDownload(GGUF);

    expect(useSettingsStore.getState().modelDownloads[RERANK]?.status).toBe('ready');
    expect(fileIpc.ollamaStatus).toHaveBeenCalledTimes(1);
  });

  it('refreshModelDownload：查询失败静默，保留映射里已有状态', async () => {
    useSettingsStore.setState({ modelDownloads: { [RERANK]: download(RERANK, 'ready') } });
    (fileIpc.modelDownloadStatus as Mock).mockRejectedValue(new Error('sidecar restarting'));

    await useSettingsStore.getState().refreshModelDownload(RERANK);

    expect(useSettingsStore.getState().modelDownloads[RERANK]?.status).toBe('ready');
  });
});
