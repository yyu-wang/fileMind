// 前端业务模型枚举与常量。
//
// 与 Rust 生成类型的关系：
// - InferenceMode / OperationType 等 enum 直接从 ipc.ts re-export，避免重复维护
// - 此文件只放前端 UI 专用的枚举（ClassifyStatus、ChatRole）与 UI 常量（色彩映射）

import type { InferenceMode } from './ipc';

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
