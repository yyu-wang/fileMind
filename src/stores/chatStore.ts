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

/** FE-C3：流空闲看门狗——上一个事件后超过该时长无新事件视为流挂起。 */
const STREAM_IDLE_TIMEOUT_MS = 90_000;
/** FE-C3：流整体时长上限（含正常长回答的余量）。 */
const STREAM_TOTAL_TIMEOUT_MS = 600_000;

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
  // FE-m11：版本号从 embeddingModelOptions 查找，fallback 1（与 Rust 当前硬编码一致）
  const version =
    settings.embeddingModelOptions.find((m) => m.name === embeddingModel)?.version ?? 1;
  const isCloud = settings.inferenceMode === 'Cloud';
  // 云端模式：若用户配置了 cloudModel，用它作为生成模型；否则回落 llmModel
  const effectiveLlmModel =
    isCloud && settings.cloudModel ? settings.cloudModel : settings.llmModel || 'qwen3.8-27b';
  return {
    query,
    history: buildHistory(messages.slice(0, -1), HISTORY_TURNS),
    table_name: `documents_${embeddingModel}_v${version}`,
    embedding_model: embeddingModel,
    inference_mode: settings.inferenceMode.toLowerCase(),
    llm_model: effectiveLlmModel,
    // 云端模型通过 llm_model 传递（Rust chat_stream_inner 会合并 cloud_model → llm_model）
    // 此处直接传 effectiveLlmModel，云端 Provider 按前缀路由
    cloud_model: isCloud ? settings.cloudModel : '',
    top_k: TOP_K,
    rerank_top_k: RERANK_TOP_K,
    max_retries: MAX_RETRIES,
    fts_chunks: [],
  };
}

// 模块级缓存：订阅一次性建立，避免 HMR / 重复调用产生多份监听
let chatUnlisten: (() => void) | null = null;

// FE-C2/FE-C3 模块级流控制状态（瞬态，不进 store 持久化）：
// - activeSeq：当前流的 seq（null = 无流 / 新流 invoke 未返回）
// - lastSeq：最近一次 invoke 返回的 seq（收编「首帧先于 invoke 返回」的事件）
// - 看门狗 timer：空闲 90s / 总量 600s 超时兜底复位 isStreaming
let activeSeq: number | null = null;
let lastSeq = 0;
let idleTimer: ReturnType<typeof setTimeout> | null = null;
let totalTimer: ReturnType<typeof setTimeout> | null = null;

/** FE-C3：清看门狗（终态/清空/发送失败时调用）。 */
function stopWatchdog() {
  if (idleTimer) clearTimeout(idleTimer);
  if (totalTimer) clearTimeout(totalTimer);
  idleTimer = null;
  totalTimer = null;
}

/**
 * FE-C3：（重新）启动看门狗。每个已处理事件都会重置空闲计时；
 * 任一超时触发时若仍在流式 → 置错误并复位全部流式状态。
 */
function restartWatchdog(onTimeout: () => void) {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = setTimeout(onTimeout, STREAM_IDLE_TIMEOUT_MS);
  if (!totalTimer) {
    totalTimer = setTimeout(onTimeout, STREAM_TOTAL_TIMEOUT_MS);
  }
}

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

        // FE-C2：invoke 返回前 activeSeq 置空——旧流残余事件（seq 为旧值）
        // 在此期间到达一律丢弃；首帧可能先于 invoke 返回，由收编逻辑处理
        activeSeq = null;

        // FE-C3：流级看门狗，超时兜底复位（done/error/clearHistory 时清除）
        restartWatchdog(() => {
          const s = get();
          if (!s.isStreaming) return;
          stopWatchdog();
          activeSeq = null;
          set({
            error: '流式响应超时，请重试',
            isStreaming: false,
            status: 'idle',
            currentStream: '',
          });
        });

        try {
          const seq = await chatStream(buildRequest(trimmed, get().messages));
          // FE-C2：seq 到手，此后只处理该 seq 的事件
          lastSeq = seq;
          activeSeq = seq;
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          stopWatchdog();
          activeSeq = null;
          set({ error: message, isStreaming: false, status: 'idle' });
        }
      },

      handleChatEvent: (event) => {
        // FE-C2：按 seq 过滤——只处理当前流的事件，旧流残余（含迟到 error）
        // 一律丢弃，避免串入新回答或误杀进行中的新流
        if (activeSeq === null) {
          // 新流 invoke 未返回：仅收编「seq 比 lastSeq 大」的首批事件
          //（旧流残余 seq 等于旧值，不会被收编）
          if (get().isStreaming && event.request_seq > lastSeq) {
            lastSeq = event.request_seq;
            activeSeq = event.request_seq;
          } else {
            return;
          }
        } else if (event.request_seq !== activeSeq) {
          return;
        }
        // FE-C3：每个已处理事件重置空闲计时
        restartWatchdog(() => {
          const s = get();
          if (!s.isStreaming) return;
          stopWatchdog();
          activeSeq = null;
          set({
            error: '流式响应超时，请重试',
            isStreaming: false,
            status: 'idle',
            currentStream: '',
          });
        });

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
          case 'search_warning':
            set({
              error: event.data.message,
              status: 'searching',
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
            activeSeq = null;
            stopWatchdog();
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
        // FE-m9：await 前占位，防止并发双调用都通过幂等检查后双注册
        if (chatUnlisten) return;
        chatUnlisten = () => {}; // 占位：后续 await 期间第二次调用会命中上行 if 直接返回
        try {
          const unlisten = await listenChatEvent((event) => get().handleChatEvent(event));
          chatUnlisten = unlisten;
        } catch (err) {
          // 注册失败：清掉占位允许重试
          chatUnlisten = null;
          console.error('chat listener 注册失败', err);
        }
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
        // 终态：流结束，清 seq 与看门狗（此后到达的同 seq 迟到事件也被过滤）
        activeSeq = null;
        stopWatchdog();
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

      clearHistory: () => {
        // FE-C2：作废在途流——清 seq 后旧流残余事件全部失效，看门狗停止
        activeSeq = null;
        stopWatchdog();
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
        });
      },

      clearError: () => set({ error: null }),
    }),
    {
      name: 'filemind-chat',
      // 仅持久化 messages（流式状态不持久化）
      partialize: (state) => ({ messages: state.messages }),
    },
  ),
);
