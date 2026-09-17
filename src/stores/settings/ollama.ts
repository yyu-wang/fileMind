// 本地 Ollama 环境探测（设置页「Ollama 环境」区块）。
//
// 与 store 分离的原因：探测带瞬态字段（ollamaProbing / lastOllamaProbeAt），且结果
// 回写三个 modelOptions 字段；模型状态变化后还要能触发刷新。
//
// 职责边界：本文件只负责「探测 + 结果写入」。模型下载与离线包导入见
// ./modelDownload.ts——自 T3 起内置 llama.cpp 引擎也是本地生成的一条路，Ollama
// 不再是唯一选项，但探测结果仍决定设置页展示，故保留独立模块。

import { fileIpc } from '@/lib/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/** Ollama 探测结果复用窗口（ms）：窗口内的进页探测直接复用上次成功结果。 */
const PROBE_TTL_MS = 60_000;

/**
 * 生成 Ollama 探测动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 *
 * Returns:
 *   探测动作（probeOllama）
 */
export function createOllamaActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<SettingsState, 'probeOllama'> {
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
  };
}
