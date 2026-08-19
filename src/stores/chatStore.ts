// 聊天 store：对话历史、流式响应（RAG 问答）。
//
// 持久化策略：messages 走 localStorage（用户关闭重开能看历史）
//
// 当前状态（RAG_ENABLED=false）：
// - sendMessage 仅 push 用户消息 + 返回功能未开放提示
// - T6.6 接入真实 RAG 流式响应后替换占位逻辑

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { FEATURE_FLAGS } from '../lib/constants/flags';
import { ChatRole, type ChatMessage } from '../types/models';

interface ChatState {
  /** 对话历史 */
  messages: ChatMessage[];
  /** 流式响应中 */
  isStreaming: boolean;
  /** 当前流式片段（逐字输出时累积） */
  currentStream: string;
  /** 错误信息 */
  error: string | null;

  /** 发送消息（RAG 关闭时仅占位） */
  sendMessage: (content: string) => Promise<void>;
  /** 追加流式片段（T6.6 用） */
  appendStreamChunk: (chunk: string) => void;
  /** 完成流式响应（T6.6 用） */
  finishStream: () => void;
  /** 清空对话历史 */
  clearHistory: () => void;
  /** 清除错误 */
  clearError: () => void;
}

/** 生成消息 ID（前端临时 ID，非持久化主键） */
function genMessageId(): string {
  return `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export const useChatStore = create<ChatState>()(
  persist(
    (set, get) => ({
      messages: [],
      isStreaming: false,
      currentStream: '',
      error: null,

      sendMessage: async (content) => {
        // 先 push 用户消息
        const userMessage: ChatMessage = {
          id: genMessageId(),
          role: ChatRole.User,
          content,
          createdAt: new Date().toISOString(),
        };
        set((state) => ({ messages: [...state.messages, userMessage], error: null }));

        if (!FEATURE_FLAGS.RAG_ENABLED) {
          // RAG 未开放：返回提示而非报错（友好降级）
          const assistantMessage: ChatMessage = {
            id: genMessageId(),
            role: ChatRole.Assistant,
            content: '知识问答功能将在后续版本开放，敬请期待。',
            createdAt: new Date().toISOString(),
          };
          set((state) => ({ messages: [...state.messages, assistantMessage] }));
          return;
        }

        // T6.6 接入真实 RAG 流式响应
        // 流程：
        // 1. set({ isStreaming: true, currentStream: '' })
        // 2. 调用 sidecar /rag/chat 流式接口
        // 3. 每收到 chunk 调 appendStreamChunk
        // 4. 完成后调 finishStream 组装完整消息
        set({ isStreaming: true, currentStream: '' });
        // TODO: T6.6 实现真实流式调用
        void get; // 避免 unused 警告，T6.6 接入后移除
        set({ isStreaming: false });
      },

      appendStreamChunk: (chunk) => {
        set((state) => ({ currentStream: state.currentStream + chunk }));
      },

      finishStream: () => {
        const stream = get().currentStream;
        if (!stream) {
          set({ isStreaming: false, currentStream: '' });
          return;
        }
        const assistantMessage: ChatMessage = {
          id: genMessageId(),
          role: ChatRole.Assistant,
          content: stream,
          createdAt: new Date().toISOString(),
        };
        set((state) => ({
          messages: [...state.messages, assistantMessage],
          isStreaming: false,
          currentStream: '',
        }));
      },

      clearHistory: () => set({ messages: [], currentStream: '', isStreaming: false }),

      clearError: () => set({ error: null }),
    }),
    {
      name: 'filemind-chat',
      // 仅持久化 messages（流式状态不持久化）
      partialize: (state) => ({ messages: state.messages }),
    },
  ),
);
