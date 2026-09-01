// 前端业务模型枚举与常量。
//
// 与 Rust 生成类型的关系：
// - InferenceMode / OperationType 等 enum 直接从 ipc.ts re-export，避免重复维护
// - 此文件只放前端 UI 专用的枚举（ClassifyStatus、ChatRole）与 UI 常量（色彩映射）

import type { CloudProvider, InferenceMode } from './ipc';

// Re-export Rust 生成的枚举，供 store / 组件直接 import 自 models 统一入口
export type { InferenceMode } from './ipc';

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
 * 云端提供商默认推理模型。
 *
 * 当 `cloudModel` 为空串时回落到此表；值与 Rust config.rs 的默认值保持一致。
 */
export const CLOUD_PROVIDER_DEFAULT_MODEL: Record<CloudProvider, string> = {
  Openai: 'gpt-4o-mini',
  Deepseek: 'deepseek-chat',
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
  cloudProvider: CloudProvider | null,
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
 * 与 Python 端 ``CITATION_PATTERN`` 语义一致的引用字面量正则。
 *
 * 用于前端 finishStream 中剥离正文中残留的引用标记（当 LLM 输出格式
 * 与解析器匹配失败时，作为最终视觉兜底）。
 * 兼容形式：``[1]`` / ``[引用1]`` / ``[引1]`` / ``[cite 2]`` / ``[来源3]`` 等。
 */
const CITATION_LITERAL_RE = /\[(?:引用|引|cite|citation|来源)?\s*(\d+)\s*\]/gi;

/**
 * 剥离正文中残留的引用字面量（[N]/[引用N] 等）。
 *
 * 引用已结构化存放在 ``ChatMessage.citations``，底部 chip 会渲染，
 * 正文中残留的 ``[引用1][引用2]`` 会是纯视觉噪音。stripCitationLiterals
 * 用于 ``finishStream`` 终态兜底：防止 Python 端漏解析（如 LLM 产生未知
 * 变体）时残留脏文本。
 *
 * @param content 原始回答正文（可能包含残留引用标记）
 * @returns 清理后的正文
 */
export function stripCitationLiterals(content: string): string {
  return (
    content
      .replace(CITATION_LITERAL_RE, '')
      // 去除引文标记后常见的残余间距：句号/问号/感叹号/逗号/分号/冒号前的空格
      .replace(/\s+([。？！，、；：,.!?;:])/g, '$1')
      .replace(/\s{2,}/g, ' ')
      .trim()
  );
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

/** 聊天消息角色。 */
export enum ChatRole {
  User = 'user',
  Assistant = 'assistant',
  System = 'system',
}

/**
 * 规则类型（对应 Rust `Rule.rule_type` 字符串字段）。
 *
 * 后端 `classifier::match_rule` 当前仅支持 extension / path_keyword / regex；
 * magic_number / size 属 E4 补充范围，表单中以禁用占位呈现（后端 `_ => false` 不匹配）。
 */
export enum RuleType {
  Extension = 'extension',
  PathKeyword = 'path_keyword',
  Regex = 'regex',
  MagicNumber = 'magic_number',
  Size = 'size',
}

/** 规则类型元数据：展示名 + 匹配模式输入提示 + 是否可用。 */
export const RULE_TYPE_META: Record<RuleType, { label: string; hint: string; disabled: boolean }> =
  {
    [RuleType.Extension]: {
      label: '扩展名',
      hint: '逗号分隔扩展名，如 pdf,doc,docx',
      disabled: false,
    },
    [RuleType.PathKeyword]: {
      label: '路径关键词',
      hint: '文件名包含的关键词，如 项目、发票',
      disabled: false,
    },
    [RuleType.Regex]: {
      label: '正则表达式',
      hint: '匹配文件名的正则，如 ^20\\d{2}',
      disabled: false,
    },
    [RuleType.MagicNumber]: {
      label: '魔数 / 文件签名',
      hint: '按文件头部字节识别（即将推出）',
      disabled: true,
    },
    [RuleType.Size]: {
      label: '文件大小',
      hint: '按文件大小阈值匹配（即将推出）',
      disabled: true,
    },
  };

/** RAG 引用（对齐 Sidecar citation 事件：id ⊆ search_result.sources.id）。 */
export interface ChatCitation {
  /** 引用来源 id（对应 search_result.sources.id） */
  id: number;
  /** 来源文件名 */
  fileName: string;
  /** 来源页码（前端预览定位依据） */
  page: number;
  /** 引用原文片段 */
  text: string;
}

/** 聊天消息（chatStore 使用）。 */
export interface ChatMessage {
  id: string;
  role: ChatRole;
  content: string;
  /** RAG 引用来源（T6.6 接入） */
  citations?: ChatCitation[];
  /** P-04 自我纠正：重试耗尽时标记低置信度 */
  lowConfidence?: boolean;
  /** P-04 自我纠正：实际重试次数 */
  retries?: number;
  createdAt: string;
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
