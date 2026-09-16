// 配置类动作：加载配置、切换推理模式、更新配置、切换本地模型、完成引导。
//
// 与 store 分离的原因：这些动作彼此强耦合（updateConfig 是多个动作的公共出口），
// 且都需要完整状态做合并（Rust 端要求每次提交完整 AppConfig）。

import { fileIpc } from '@/lib/ipc';
import type { AppConfig, InferenceMode } from '@/types/ipc';
import type { SettingsGet, SettingsSet, SettingsState } from './types';

/**
 * 本地生成后端与内置 GGUF 标识的默认值。
 *
 * 三处静态默认值必须一致：本模块、`src-tauri/src/commands/config.rs`
 * （`AppConfig::default`）、`V019__add_local_llm_backend.sql` 的列默认值。
 * 后端 AppConfig 对这两个字段声明了 serde 默认（与 cloud_model 同款防御），
 * 生成类型里是可选字段，故读取时需在此兜底。
 */
export const DEFAULT_LOCAL_LLM_BACKEND = 'ollama';
export const DEFAULT_LOCAL_LLM_MODEL = 'qwen2.5-3b-instruct';

/**
 * 取值优先级：partial 显式给出（**含 null**）→ 当前状态值。
 *
 * 抽成函数而非内联 `??`：15 个字段的 `??` 链会把 mergeAppConfig 的圈复杂度顶到 17
 * （阈值 15），且 `??` 与 `||` 的 null 语义在这里必须显式区分——partial 传 null 表示
 * 「显式置空」（如撤销云端同意），不能被当成"未提供"。
 */
function pick<T>(fromPartial: T | undefined, current: T): T {
  return fromPartial === undefined ? current : fromPartial;
}

/**
 * 把「部分字段更新」合并成 Rust 端要求的完整 AppConfig。
 *
 * ⚠️ 兜底必须回填当前值而非 null：后端 upsert 是全字段覆盖，置空会静默清掉已签署的
 * 同意书记录（合规审计链断裂）。新增配置字段时必须在此登记，否则任何无关的配置更新
 * 都会把它写回默认值（历史回归：切后端被静默重置）。
 */
function mergeAppConfig(current: SettingsState, partial: Partial<AppConfig>): AppConfig {
  return {
    data_directory: pick(partial.data_directory, current.dataDirectory),
    inference_mode: pick(partial.inference_mode, current.inferenceMode.toLowerCase()),
    embedding_model: pick(partial.embedding_model, current.embeddingModel),
    llm_model: pick(partial.llm_model, current.llmModel),
    max_file_size_mb: pick(partial.max_file_size_mb, current.maxFileSizeMb),
    language: pick(partial.language, current.language),
    onboarding_completed: pick(partial.onboarding_completed, current.onboardingCompleted),
    cloud_consent_signed: pick(partial.cloud_consent_signed, current.cloudConsentSigned),
    cloud_consent_version: pick(partial.cloud_consent_version, current.cloudConsentVersion),
    cloud_consent_provider: pick(partial.cloud_consent_provider, current.cloudConsentProvider),
    cloud_consent_signed_at: pick(partial.cloud_consent_signed_at, current.cloudConsentSignedAt),
    cloud_model: pick(partial.cloud_model, current.cloudModel),
    active_cloud_provider: pick(partial.active_cloud_provider, current.activeCloudProvider) || null,
    local_llm_backend: pick(partial.local_llm_backend, current.localLlmBackend),
    local_llm_model: pick(partial.local_llm_model, current.localLlmModel),
  };
}

/**
 * AppConfig → 前端 state 字段映射（loadConfig 与 updateConfig 成功路径共用）。
 *
 * 共用一份的原因：两处映射必须逐字段一致，此前是两份手抄的字段列表，
 * 新增配置字段很容易只改一处（表现为「加载能读到、更新后丢失」）。
 */
function stateFromConfig(cfg: AppConfig): Partial<SettingsState> {
  return {
    dataDirectory: cfg.data_directory,
    inferenceMode: normalizeInferenceMode(cfg.inference_mode),
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
    // 后端声明了 serde 默认（生成类型里是可选字段），故读取时兜底
    localLlmBackend: cfg.local_llm_backend ?? DEFAULT_LOCAL_LLM_BACKEND,
    localLlmModel: cfg.local_llm_model ?? DEFAULT_LOCAL_LLM_MODEL,
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
