// 配置类动作：加载配置、切换推理模式、更新配置、切换本地模型、完成引导。
//
// 与 store 分离的原因：这些动作彼此强耦合（updateConfig 是多个动作的公共出口），
// 且都需要完整状态做合并（Rust 端要求每次提交完整 AppConfig）。

import { fileIpc } from '@/lib/ipc';
import type { AppConfig, InferenceMode } from '@/types/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/**
 * 把「部分字段更新」合并成 Rust 端要求的完整 AppConfig。
 *
 * 抽为独立函数的原因：13 个字段的兜底合并若全挤在 updateConfig 内联，会把它的圈复杂度
 * 顶到 18（complexity 阈值为 15）；合并本身是内聚的一步，独立后两个函数都在阈值内。
 *
 * ⚠️ 兜底必须回填当前值而非 null：后端 upsert 是全字段覆盖，置空会静默清掉已签署的
 * 同意书记录（合规审计链断裂）。
 */
function mergeAppConfig(current: SettingsState, partial: Partial<AppConfig>): AppConfig {
  return {
    data_directory: partial.data_directory ?? current.dataDirectory,
    inference_mode: partial.inference_mode ?? current.inferenceMode.toLowerCase(),
    embedding_model: partial.embedding_model ?? current.embeddingModel,
    llm_model: partial.llm_model ?? current.llmModel,
    max_file_size_mb: partial.max_file_size_mb ?? current.maxFileSizeMb,
    language: partial.language ?? current.language,
    onboarding_completed: partial.onboarding_completed ?? current.onboardingCompleted,
    cloud_consent_signed: partial.cloud_consent_signed ?? current.cloudConsentSigned,
    cloud_consent_version: partial.cloud_consent_version ?? current.cloudConsentVersion,
    cloud_consent_provider: partial.cloud_consent_provider ?? current.cloudConsentProvider,
    cloud_consent_signed_at: partial.cloud_consent_signed_at ?? current.cloudConsentSignedAt,
    cloud_model: partial.cloud_model ?? current.cloudModel,
    active_cloud_provider: (partial.active_cloud_provider ?? current.activeCloudProvider) || null,
  };
}

/**
 * 将 Rust 端字符串推理模式归一化为 InferenceMode 枚举值。
 *
 * Rust AppConfig.inference_mode 是 string 类型（可能 'local' / 'cloud'），
 * 此处统一转 PascalCase 对齐 InferenceMode 枚举。
 */
function normalizeInferenceMode(raw: string): InferenceMode {
  const lower = raw.toLowerCase();
  if (lower === 'cloud') return 'Cloud';
  // 默认 Local（含未知值兜底，避免前端崩溃）
  return 'Local';
}

/**
 * FE-M11：loadConfig 的实际加载逻辑（被 loadConfig 的 try/catch 包裹）。
 * 抽为独立函数：store action 内联时替换残留体困难，独立函数更清晰。
 * 业务错误（status==='error'）与 IPC 异常统一 throw，由调用方落 initFailed。
 */
async function loadConfigInner(set: SettingsSet): Promise<void> {
  const result = await fileIpc.getConfig();
  if (result.status === 'ok') {
    const cfg = result.data;
    const mode = normalizeInferenceMode(cfg.inference_mode);
    set({
      dataDirectory: cfg.data_directory,
      inferenceMode: mode,
      embeddingModel: cfg.embedding_model,
      maxFileSizeMb: cfg.max_file_size_mb,
      language: cfg.language,
      onboardingCompleted: cfg.onboarding_completed,
      cloudConsentSigned: cfg.cloud_consent_signed,
      cloudConsentVersion: cfg.cloud_consent_version,
      cloudConsentProvider: cfg.cloud_consent_provider,
      cloudConsentSignedAt: cfg.cloud_consent_signed_at,
      llmModel: cfg.llm_model,
      cloudModel: cfg.cloud_model ?? '',
      activeCloudProvider: cfg.active_cloud_provider ?? '',
      isLoading: false,
      initFailed: false,
    });
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
      set({
        dataDirectory: fullConfig.data_directory,
        inferenceMode: normalizeInferenceMode(fullConfig.inference_mode),
        embeddingModel: fullConfig.embedding_model,
        maxFileSizeMb: fullConfig.max_file_size_mb,
        language: fullConfig.language,
        onboardingCompleted: fullConfig.onboarding_completed,
        cloudConsentSigned: fullConfig.cloud_consent_signed,
        cloudConsentVersion: fullConfig.cloud_consent_version,
        cloudConsentProvider: fullConfig.cloud_consent_provider,
        cloudConsentSignedAt: fullConfig.cloud_consent_signed_at,
        llmModel: fullConfig.llm_model,
        cloudModel: fullConfig.cloud_model ?? '',
        activeCloudProvider: fullConfig.active_cloud_provider ?? '',
      });
    },

    completeOnboarding: async (dataDirectory) => {
      // 引导完成：更新 data_directory + onboarding_completed=true
      await get().updateConfig({ data_directory: dataDirectory, onboarding_completed: true });
    },
  };
}
