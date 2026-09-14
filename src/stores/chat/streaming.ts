// 聊天流式动作：发送消息、事件分发入口、追加片段、收尾组装。
//
// 与 store 分离的原因：这些是「流式链路」的核心动作（含 IPC 调用与看门狗），
// 依赖通过 deps 注入（set/get）而非直接引用 store，避免循环依赖并便于单测。

import { chatStream } from '@/lib/ipc/chatIpc';
import { flushThrottledStorage } from '@/lib/throttledStorage';
import { ChatRole, stripCitationLiterals, type ChatMessage } from '@/types/models';
import { dispatchChatEvent } from './events';
import { genMessageId } from './messages';
import { buildRequest } from './request';
import {
  acceptEvent,
  resetActiveSeq,
  restartWatchdog,
  setActiveSeq,
  stopWatchdog,
} from './streamControl';
import type { ChatGet, ChatSet, ChatState } from './types';

/** 流式动作工厂所需的最小依赖面。 */
export interface StreamingDeps {
  /** 写入状态（zustand 的 set） */
  set: ChatSet;
  /** 读当前状态（zustand 的 get） */
  get: ChatGet;
}

/**
 * 生成流式动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set/get
 *
 * Returns:
 *   发送 / 事件入口 / 追加片段 / 收尾四个动作
 */
export function createStreamingActions({
  set,
  get,
}: StreamingDeps): Pick<
  ChatState,
  'sendMessage' | 'handleChatEvent' | 'appendStreamChunk' | 'finishStream'
> {
  return {
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
      resetActiveSeq();

      // FE-C3：流级看门狗，超时兜底复位（done/error/clearHistory 时清除）
      restartWatchdog(() => {
        const s = get();
        if (!s.isStreaming) return;
        stopWatchdog();
        resetActiveSeq();
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
        setActiveSeq(seq);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        stopWatchdog();
        resetActiveSeq();
        set({ error: message, isStreaming: false, status: 'idle' });
      }
    },

    handleChatEvent: (event) => {
      // FE-C2：按 seq 过滤——只处理当前流的事件，旧流残余（含迟到 error）
      // 一律丢弃，避免串入新回答或误杀进行中的新流（判定见 streamControl.acceptEvent）
      if (!acceptEvent(event.request_seq, get().isStreaming)) return;
      // FE-C3：每个已处理事件重置空闲计时
      restartWatchdog(() => {
        const s = get();
        if (!s.isStreaming) return;
        stopWatchdog();
        resetActiveSeq();
        set({
          error: '流式响应超时，请重试',
          isStreaming: false,
          status: 'idle',
          currentStream: '',
        });
      });

      dispatchChatEvent(event, { set, get });
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
      resetActiveSeq();
      stopWatchdog();
      if (!currentStream) {
        // 空流（异常终止）：不产生空消息，仅复位状态
        set({ isStreaming: false, status: 'idle', currentStream: '' });
        return;
      }
      const assistantMessage: ChatMessage = {
        id: genMessageId(),
        role: ChatRole.Assistant,
        content: stripCitationLiterals(currentStream),
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
      // 终态消息立即落盘，不等节流窗口
      flushThrottledStorage();
    },
  };
}
