// 配置的默认值与双向映射：partial → 完整 AppConfig、AppConfig → 前端 state。
//
// 与 config.ts 分离的原因：这些是纯变换（无 IPC、无 store 依赖），却占了原文件的
// 一半篇幅；拆出后 config.ts 只留动作编排（complexity 规则：.ts 警告 150 行）。

import type { AppConfig, InferenceMode } from '@/types/ipc';
import type { SettingsState } from './types';

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
export function mergeAppConfig(current: SettingsState, partial: Partial<AppConfig>): AppConfig {
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
export function stateFromConfig(cfg: AppConfig): Partial<SettingsState> {
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
