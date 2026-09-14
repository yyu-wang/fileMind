// 本地 Ollama 相关动作：环境探测（含 TTL 节流）与 Embedding 模型安装。
//
// 与 store 分离的原因：两者共享同一组瞬态字段（ollamaProbing / installingModel /
// lastOllamaProbeAt），且安装成功后需要强制绕过探测节流刷新列表。

import { fileIpc } from '@/lib/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/** Ollama 探测结果复用窗口（ms）：窗口内的进页探测直接复用上次成功结果。 */
const PROBE_TTL_MS = 60_000;

/**
 * 生成 Ollama 相关动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 *
 * Returns:
 *   探测与模型安装两个动作
 */
export function createOllamaActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<SettingsState, 'probeOllama' | 'installModel'> {
  const { set, get } = deps;
  return {
    probeOllama: async (force = false) => {
      // TTL 节流：60s 内已成功探测则复用（探测是真实 HTTP 往返，进出设置页
      // 反复触发会明显拖慢切页）。失败不缓存（lastOllamaProbeAt 不更新），
      // 下次调用照常重探；force 绕过节流（重新检测按钮 / 安装模型后）。
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

    installModel: async (modelName) => {
      set({ installingModel: modelName, installError: null });
      try {
        const result = await fileIpc.installEmbeddingModel(modelName);
        if (result.status === 'ok') {
          if (result.data.success) {
            // 模型列表已变，强制绕过探测节流
            await get().probeOllama(true);
          } else {
            set({ installError: result.data.message });
          }
        } else {
          set({ installError: result.error });
        }
      } catch (e) {
        set({ installError: e instanceof Error ? e.message : String(e) });
      } finally {
        set({ installingModel: null });
      }
    },
  };
}
