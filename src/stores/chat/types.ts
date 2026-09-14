// 聊天 store 的公共类型、状态接口与初始状态：供 store 与同目录子模块共用。
//
// 独立成文件的原因：`ChatState` 被 store 与各 action 工厂（streaming/listener/lifecycle）
// 共同引用，放在 store 里会让子模块反向依赖 store（形成循环导入）。
// `ChatSet` / `ChatGet` 是工厂注入的 set/get 依赖面，签名对齐 zustand。

import type { ChatEvent } from '@/lib/ipc/chatIpc';
import type { ChatCitation, ChatMessage } from '@/types/models';

/** 聊天流程状态（SearchStatusBar 展示用）。 */
export type ChatStatus = 'idle' | 'searching' | 'streaming';

/** 检索状态（`search_result` 事件载荷，UI 状态栏展示用）。 */
export interface ChatSearchInfo {
  candidates: number;
  afterRerank: number;
  sources: Array<{ id: number; fileName: string; page: number; score: number }>;
}

/** 流式收尾元信息（`done` 事件后组装 assistant 消息时消费）。 */
export interface StreamMeta {
  /** P-04 重试耗尽标记（来自 `done.low_confidence`）。 */
  lowConfidence: boolean;
}

/** 聊天 store 的状态与动作集合。 */
export interface ChatState {
  /** 对话历史 */
  messages: ChatMessage[];
  /** 流式响应中 */
  isStreaming: boolean;
  /** 当前流式片段（逐字输出累积，retry 时清空） */
  currentStream: string;
  /** 错误信息 */
  error: string | null;
  /** 聊天流程状态 */
  status: ChatStatus;
  /** 改写后的查询（`search_start` 事件） */
  rewrittenQuery: string | null;
  /** 检索状态（`search_result` 事件） */
  searchInfo: ChatSearchInfo | null;
  /** P-04 实际重试次数 */
  retries: number;
  /** P-04 最近一次重试原因 */
  retryReason: string | null;
  /** P-04 低置信度标记 */
  lowConfidence: boolean;
  /** 已到达的引用（`citation` 事件，finishStream 组装消息时消费） */
  pendingCitations: ChatCitation[];

  /** 发送消息：推入用户消息 → 发起流式请求 */
  sendMessage: (content: string) => Promise<void>;
  /** 分发 `chat://event` 事件（纯逻辑，便于单测） */
  handleChatEvent: (event: ChatEvent) => void;
  /** 订阅 `chat://event`（main.tsx 启动时调用，幂等） */
  initChatListener: () => Promise<void>;
  /** 追加流式片段 */
  appendStreamChunk: (chunk: string) => void;
  /** 完成流式响应：组装 assistant 消息（含引用/低置信度/重试标记） */
  finishStream: (meta: StreamMeta) => void;
  /** 清空对话历史 */
  clearHistory: () => void;
  /** 清除错误 */
  clearError: () => void;
}

/** 初始状态（store 初始化与测试复位共用同一语义）。 */
export const INITIAL_CHAT_STATE: Pick<
  ChatState,
  | 'messages'
  | 'isStreaming'
  | 'currentStream'
  | 'error'
  | 'status'
  | 'rewrittenQuery'
  | 'searchInfo'
  | 'retries'
  | 'retryReason'
  | 'lowConfidence'
  | 'pendingCitations'
> = {
  messages: [],
  isStreaming: false,
  currentStream: '',
  error: null,
  status: 'idle',
  rewrittenQuery: null,
  searchInfo: null,
  retries: 0,
  retryReason: null,
  lowConfidence: false,
  pendingCitations: [],
};

/** 动作工厂注入的写状态依赖（对齐 zustand set：支持对象补丁与函数式更新）。 */
export type ChatSet = (
  partial: Partial<ChatState> | ((state: ChatState) => Partial<ChatState>),
) => void;

/** 动作工厂注入的读状态依赖（对齐 zustand get）。 */
export type ChatGet = () => ChatState;
