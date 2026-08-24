// 聊天 store：对话历史 + RAG 流式响应（T6.6）。
//
// 流式链路：
//   sendMessage → chatStream(request)（Rust 代理 Sidecar /chat/stream）
//   Rust 后台逐帧解析 SSE → 经 chat://event 推送 → initChatListener 订阅
//   → handleChatEvent 按事件分发：
//     search_start → searching | search_result → searchInfo | token → 追加
//     retry → 清空缓冲重推（T5.7 契约）| citation → 暂存 | done → 组装消息 | error → 置错
//
// 持久化策略：messages 走 localStorage（关闭重开可看历史）；
// 流式状态（status/currentStream 等）为瞬态不持久化。

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { chatStream, listenChatEvent, type ChatEvent } from '../lib/ipc/chatIpc';
import { useSettingsStore } from './settingsStore';
import { ChatRole, type ChatCitation, type ChatMessage } from '../types/models';
import type { ChatStreamRequest, ChatTurn } from '../types/ipc';

/** 向量检索候选数。 */
const TOP_K = 20;
/** 重排后保留数。 */
const RERANK_TOP_K = 5;
/** P-04 自我纠正最大重试次数。 */
const MAX_RETRIES = 2;
/** 透传给查询改写的对话轮数（对齐 P-02 `HISTORY_TURNS=3`）。 */
const HISTORY_TURNS = 3;

/** 聊天流程状态（SearchStatusBar 展示用）。 */
export type ChatStatus = 'idle' | 'searching' | 'streaming';

/** 检索状态（`search_result` 事件载荷，UI 状态栏展示用）。 */
export interface ChatSearchInfo {
  candidates: number;
  afterRerank: number;
  sources: Array<{ id: number; fileName: string; page: number; score: number }>;
}

interface StreamMeta {
  /** P-04 重试耗尽标记（来自 `done.low_confidence`）。 */
  lowConfidence: boolean;
}

interface ChatState {
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

/** 生成消息 ID（前端临时 ID，非持久化主键）。 */
function genMessageId(): string {
  return `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 取最近 `maxTurns` 轮 user/assistant 对话对（P-02 查询改写输入）。 */
function buildHistory(messages: ChatMessage[], maxTurns: number): ChatTurn[] {
  const turns: ChatTurn[] = [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const assistant = messages[i];
    if (assistant.role !== ChatRole.Assistant) continue;
    const user = messages[i - 1];
    if (!user || user.role !== ChatRole.User) continue;
    turns.push({ user: user.content, assistant: assistant.content });
    i--;
    if (turns.length >= maxTurns) break;
  }
  return turns.reverse();
}

/**
 * 构造 `/chat/stream` 请求体。
 *
 * `messages` 已包含刚发送的 user 消息，历史取其之前的最近 N 轮；
 * `embedding_model` 来自 settingsStore，`table_name` 对齐 LanceDB 命名
 * `documents_{embedding_model}_v{version}`。`fts_chunks` 恒空（Rust 侧
 * 尚无 chunk 级 FTS5，建索引为后续独立任务）。
 */
function buildRequest(query: string, messages: ChatMessage[]): ChatStreamRequest {
  const settings = useSettingsStore.getState();
  const embeddingModel = settings.embeddingModel;
  return {
    query,
    history: buildHistory(messages.slice(0, -1), HISTORY_TURNS),
    table_name: `documents_${embeddingModel}_v1`,
    embedding_model: embeddingModel,
    inference_mode: settings.inferenceMode.toLowerCase(),
    llm_model: settings.llmModel || 'qwen3.8-27b',
    top_k: TOP_K,
    rerank_top_k: RERANK_TOP_K,
    max_retries: MAX_RETRIES,
    fts_chunks: [],
  };
}

// 模块级缓存：订阅一次性建立，避免 HMR / 重复调用产生多份监听
let chatUnlisten: (() => void) | null = null;

export const useChatStore = create<ChatState>()(
  persist(
    (set, get) => ({
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

      sendMessage: async (content) => {
        const trimmed = content.trim();
        if (!trimmed || get().isStreaming) return;

        const userMessage: ChatMessage = {
          id: genMessageId(),
          role: ChatRole.User,
          content: trimmed,
          createdAt: new Date().toISOString(),
        };
        set((state) => ({
          messages: [...state.messages, userMessage],
          isStreaming: true,
          status: 'searching',
          currentStream: '',
          pendingCitations: [],
          rewrittenQuery: null,
          searchInfo: null,
          retries: 0,
          retryReason: null,
          lowConfidence: false,
          error: null,
        }));

        try {
          await chatStream(buildRequest(trimmed, get().messages));
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          set({ error: message, isStreaming: false, status: 'idle' });
        }
      },

      handleChatEvent: (event) => {
        const { isStreaming } = get();
        // 非流式会话期间的迟到事件忽略（error 除外，便于 UI 透出侧车错误）
        if (!isStreaming && event.event !== 'error') return;

        switch (event.event) {
          case 'search_start':
            set({
              status: 'searching',
              rewrittenQuery: event.data.query_rewritten,
              error: null,
            });
            break;
          case 'search_result':
            set({
              searchInfo: {
                candidates: event.data.candidates,
                afterRerank: event.data.after_rerank,
                sources: event.data.sources.map((s) => ({
                  id: s.id,
                  fileName: s.file_name,
                  page: s.page,
                  score: s.score,
                })),
              },
            });
            break;
          case 'token':
            get().appendStreamChunk(event.data.content);
            break;
          case 'retry':
            // T5.7 契约：修正答案重推前清空当前缓冲，前端从零重绘
            set({
              currentStream: '',
              retries: event.data.attempt,
              retryReason: event.data.reason,
              status: 'streaming',
            });
            break;
          case 'citation':
            set({
              pendingCitations: event.data.citations.map((c) => ({
                id: c.id,
                fileName: c.file_name,
                page: c.page,
                text: c.text,
              })),
            });
            break;
          case 'done':
            get().finishStream({ lowConfidence: event.data.low_confidence ?? false });
            break;
          case 'error':
            set({
              error: event.data.message,
              isStreaming: false,
              status: 'idle',
              currentStream: '',
            });
            break;
        }
      },

      initChatListener: async () => {
        if (chatUnlisten) return;
        chatUnlisten = await listenChatEvent((event) => get().handleChatEvent(event));
      },

      appendStreamChunk: (chunk) => {
        set((state) => ({
          currentStream: state.currentStream + chunk,
          status: 'streaming',
          isStreaming: true,
        }));
      },

      finishStream: (meta) => {
        const { currentStream, pendingCitations, retries } = get();
        if (!currentStream) {
          // 空流（异常终止）：不产生空消息，仅复位状态
          set({ isStreaming: false, status: 'idle', currentStream: '' });
          return;
        }
        const assistantMessage: ChatMessage = {
          id: genMessageId(),
          role: ChatRole.Assistant,
          content: currentStream,
          ...(pendingCitations.length > 0 ? { citations: pendingCitations } : {}),
          ...(meta.lowConfidence ? { lowConfidence: true } : {}),
          ...(retries > 0 ? { retries } : {}),
          createdAt: new Date().toISOString(),
        };
        set((state) => ({
          messages: [...state.messages, assistantMessage],
          isStreaming: false,
          status: 'idle',
          currentStream: '',
          pendingCitations: [],
          retries: 0,
          retryReason: null,
          rewrittenQuery: null,
          searchInfo: null,
          lowConfidence: false,
        }));
      },

      clearHistory: () =>
        set({
          messages: [],
          currentStream: '',
          isStreaming: false,
          status: 'idle',
          pendingCitations: [],
          retries: 0,
          retryReason: null,
          rewrittenQuery: null,
          searchInfo: null,
          lowConfidence: false,
        }),

      clearError: () => set({ error: null }),
    }),
    {
      name: 'filemind-chat',
      // 仅持久化 messages（流式状态不持久化）
      partialize: (state) => ({ messages: state.messages }),
    },
  ),
);
