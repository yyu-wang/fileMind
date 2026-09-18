// 配置类动作：加载配置、切换推理模式、更新配置、切换本地模型、完成引导。
//
// 纯映射与默认值拆到 ./configMapping.ts（无 IPC、无 store 依赖），本文件只做动作编排。
//
// 与 store 分离的原因：这些动作彼此强耦合（updateConfig 是多个动作的公共出口），
// 且都需要完整状态做合并（Rust 端要求每次提交完整 AppConfig）。

import { fileIpc } from '@/lib/ipc';
import { mergeAppConfig, stateFromConfig } from './configMapping';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/**
 * FE-M11：loadConfig 的实际加载逻辑（被 loadConfig 的 try/catch 包裹）。
 * 抽为独立函数：store action 内联时替换残留体困难，独立函数更清晰。
 * 业务错误（status==='error'）与 IPC 异常统一 throw，由调用方落 initFailed。
 */
async function loadConfigInner(set: SettingsSet): Promise<void> {
  const result = await fileIpc.getConfig();
  if (result.status === 'ok') {
    set({ ...stateFromConfig(result.data), isLoading: false, initFailed: false });
  } else {
    throw new Error(result.error);
  }
}

/**
 * 生成配置类动作（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set / get
 *
 * Returns:
 *   配置类动作集合
 */
export function createConfigActions(deps: {
  set: SettingsSet;
  get: SettingsGet;
}): Pick<
  SettingsState,
  | 'loadConfig'
  | 'retryInit'
  | 'setInferenceMode'
  | 'setLlmModel'
  | 'updateConfig'
  | 'completeOnboarding'
> {
  const { set, get } = deps;
  return {
    loadConfig: async () => {
      set({ isLoading: true, error: null });
      // FE-M11：typedError 对 Error 实例直接 rethrow（specta 生成，不可改），
      // 必须兜底——否则 isLoading 永久 true，App 渲染「正在加载配置...」白屏
      try {
        await loadConfigInner(set);
      } catch (e) {
        set({
          isLoading: false,
          initFailed: true,
          error: e instanceof Error ? e.message : String(e),
        });
      }
    },

    retryInit: async () => {
      await get().loadConfig();
    },

    setInferenceMode: async (mode) => {
      // source='ui' 标识来自前端用户主动操作（04 API §2-3b 安全约束）
      const result = await fileIpc.setInferenceMode(mode, 'ui');
      if (result.status === 'ok') {
        // 模式切换不改变用户已选的 LLM 模型（llmModel 保持真实模型名）
        set({ inferenceMode: mode });
      } else {
        set({ error: result.error });
        throw new Error(result.error);
      }
    },

    setLlmModel: async (name) => {
      // FE-M5：乐观更新 + 失败回滚（此前失败不回滚，下拉显示未持久化的模型）
      const prev = get().llmModel;
      set({ llmModel: name });
      try {
        await get().updateConfig({ llm_model: name });
      } catch (e) {
        // 仅当 store 值仍是本次乐观值时回滚：防并发场景下误伤其他更新已落的新值
        if (get().llmModel === name) {
          set({ llmModel: prev, error: e instanceof Error ? e.message : String(e) });
        }
        throw e;
      }
    },

    updateConfig: async (partial) => {
      // Rust 端要求完整 AppConfig，先合并当前值再发送
      const fullConfig = mergeAppConfig(get(), partial);
      const result = await fileIpc.updateConfig(fullConfig);
      if (result.status !== 'ok') {
        set({ error: result.error });
        throw new Error(result.error);
      }
      // 成功：发送的 fullConfig 就是后端 upsert 落库的全字段真值，直接本地合并。
      // 不再全量 getConfig 重拉——省一次串行 IPC，且 loadConfig 会置
      // isLoading 让整个 App 闪「正在加载配置」加载态（一次模型下拉选择即触发）。
      set(stateFromConfig(fullConfig));
    },

    completeOnboarding: async (dataDirectory) => {
      // 引导完成：更新 data_directory + onboarding_completed=true
      await get().updateConfig({ data_directory: dataDirectory, onboarding_completed: true });
    },
  };
}
