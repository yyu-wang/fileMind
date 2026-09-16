// 本地 Ollama 环境探测 + Embedding 模型下载动作。
//
// 与 store 分离的原因：两者共享同一组瞬态字段（ollamaProbing / downloadingModel /
// modelDownload / lastOllamaProbeAt），且探测在模型状态变化后需要刷新。
//
// 职责边界：Ollama 只负责「本地生成模型」（探测）；Embedding 模型由 Sidecar 从
// HF 镜像下载（见 python-sidecar/app/services/model_download_service.py），
// 本文件只负责触发与查询状态，进度由设置页按固定间隔轮询。

import { fileIpc } from '@/lib/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/** Ollama 探测结果复用窗口（ms）：窗口内的进页探测直接复用上次成功结果。 */
const PROBE_TTL_MS = 60_000;

/**
 * 生成 Ollama 探测与模型下载相关动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 *
 * Returns:
 *   探测、启动下载、查询下载状态三个动作
 */
export function createOllamaActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<SettingsState, 'probeOllama' | 'startModelDownload' | 'refreshModelDownload'> {
  const { set, get } = deps;
  return {
    probeOllama: async (force = false) => {
      // TTL 节流：60s 内已成功探测则复用（探测是真实 HTTP 往返，进出设置页
      // 反复触发会明显拖慢切页）。失败不缓存（lastOllamaProbeAt 不更新），
      // 下次调用照常重探；force 绕过节流（重新检测按钮 / 模型状态变化后）。
      if (
        !force &&
        get().ollamaStatus !== null &&
        Date.now() - get().lastOllamaProbeAt < PROBE_TTL_MS
      ) {
        return;
      }
      set({ ollamaProbing: true, error: null });
      try {
        const result = await fileIpc.ollamaStatus();
        if (result.status === 'ok') {
          set({
            ollamaStatus: result.data,
            llmModelOptions: result.data.llm_models,
            embeddingModelOptions: result.data.embedding_models,
            lastOllamaProbeAt: Date.now(),
          });
        } else {
          set({ ollamaStatus: null, error: result.error });
        }
      } finally {
        set({ ollamaProbing: false });
      }
    },

    startModelDownload: async (modelName) => {
      // Sidecar 侧下载是后台任务：本调用立即返回起始状态，进度靠轮询
      set({ downloadingModel: modelName, installError: null });
      try {
        const result = await fileIpc.startModelDownload(modelName);
        if (result.status === 'ok') {
          set({ modelDownload: result.data });
        } else {
          set({ installError: result.error, downloadingModel: null });
        }
      } catch (e) {
        set({
          installError: e instanceof Error ? e.message : String(e),
          downloadingModel: null,
        });
      }
    },

    refreshModelDownload: async (modelName) => {
      // 轮询路径：查询失败静默保持上次状态（Sidecar 可能正在重启；下载失败会由
      // status=failed 携带 error，无需在这里额外标记）
      try {
        const result = await fileIpc.modelDownloadStatus(modelName);
        if (result.status !== 'ok') {
          return;
        }
        const status = result.data;
        const wasReady = get().modelDownload?.status === 'ready';
        set({
          modelDownload: status,
          downloadingModel: status.status === 'downloading' ? modelName : null,
        });
        // 首次转为 ready → 模型就绪状态变了，绕过节流刷新探测（可用性标签随之更新）
        if (status.status === 'ready' && !wasReady) {
          await get().probeOllama(true);
        }
      } catch {
        // 轮询异常同样静默：下一次轮询会重试
      }
    },
  };
}
