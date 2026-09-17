// 前端业务模型枚举与常量（前端模型的统一入口）。
//
// 与 Rust 生成类型的关系：
// - InferenceMode / OperationType 等 enum 直接从 ipc.ts re-export，避免重复维护
// - 此文件只放前端 UI 专用的枚举（ClassifyStatus、ThemeMode）与 UI 常量（色彩映射）
//
// 模块划分（2026-09-17：原单文件 200 行逼近 .ts 强制阈值 250）：
// - ./chat.ts   聊天消息 / RAG 引用 / 引用字面量清洗
// - ./rules.ts  规则类型枚举与表单元数据
// 两者在此再导出——本文件是前端模型的统一入口（54 处
// `import ... from '@/types/models'` 无需改动），与下方 InferenceMode 的再导出一致。

import type { InferenceMode } from './ipc';

// Re-export Rust 生成的枚举，供 store / 组件直接 import 自 models 统一入口
export type { InferenceMode } from './ipc';
// Re-export 拆出的子模块（同上，保持统一入口）
export { ChatRole, stripCitationLiterals } from './chat';
export type { ChatCitation, ChatMessage } from './chat';
export { RULE_TYPE_META, RuleType } from './rules';

/**
 * 推理模式色彩标识（设计稿 9.2 推理模式色彩标识）。
 *
 * 当前仅 Local / Cloud 两种（Rust InferenceMode 枚举）；
 * Hybrid 模式留待 T11 安全沙箱扩展后再补色。
 */
export const MODE_COLORS: Record<
  InferenceMode,
  { bg: string; label: string; dot: string; model: string }
> = {
  Local: { bg: '#e9d8fd', label: '本地模式', dot: '#6b46c1', model: 'Ollama · Qwen2.5:7B' },
  Cloud: { bg: '#bee3f8', label: '云端模式', dot: '#2b6cb0', model: 'OpenAI · gpt-4o-mini' },
};

/**
 * 云端提供商默认推理模型（按 provider_key 取值）。
 *
 * 当 `cloudModel` 为空串时回落到此表；键为 P-07 提供商 slug。
 * 内置两条（openai / deepseek）历史默认保留；用户自定义提供商为空走回落逻辑。
 */
export const CLOUD_PROVIDER_DEFAULT_MODEL: Record<string, string> = {
  openai: 'gpt-4o-mini',
  deepseek: 'deepseek-chat',
};

/**
 * 解析状态栏/会话实际使用的 LLM 模型显示名。
 *
 * 逻辑（与 chatStore buildRequest L125-127 对齐）：
 * - Local：`llmModel`（本地 Ollama 模型名，空值回落 qwen3.8-27b）
 * - Cloud：优先 `cloudModel`，空则按 `cloudProvider` 查默认，均无回落 llmModel
 *
 * @param mode 推理模式
 * @param llmModel 本地 LLM 模型名（store.llmModel）
 * @param cloudModel 云端推理模型名（store.cloudModel，空串=未指定）
 * @param cloudProvider 云端提供商（仅 Cloud 模式有值）
 */
export function resolveDisplayModel(
  mode: InferenceMode,
  llmModel: string | undefined | null,
  cloudModel: string | undefined | null,
  cloudProvider: string | null,
): string {
  const isCloud = mode === 'Cloud';
  if (isCloud) {
    if (cloudModel && cloudModel.trim() !== '') {
      return cloudModel;
    }
    if (cloudProvider) {
      return CLOUD_PROVIDER_DEFAULT_MODEL[cloudProvider];
    }
  }
  return llmModel && llmModel.trim() !== '' ? llmModel : 'qwen3.8-27b';
}

/**
 * 分类流程状态机（设计稿 5.x 分类页）。
 *
 * 状态转移：
 *   Idle → Previewing → Running ⇄ Paused → Done | Cancelled
 */
export enum ClassifyStatus {
  Idle = 'idle',
  Previewing = 'previewing',
  Running = 'running',
  Paused = 'paused',
  Done = 'done',
  Cancelled = 'cancelled',
}

/**
 * 主题模式（T6.9 暗色模式）。
 *
 * 三态：System（跟随系统，纯 CSS 响应）/ Light（强制亮色）/ Dark（强制暗色）。
 * 持久化于 settingsStore，启动时经 applyTheme 应用到 `<html data-theme>`。
 */
export enum ThemeMode {
  System = 'system',
  Light = 'light',
  Dark = 'dark',
}
