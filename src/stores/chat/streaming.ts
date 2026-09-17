// 聊天流式动作：发送消息、事件分发入口、追加片段、收尾组装。
//
// 与 store 分离的原因：这些是「流式链路」的核心动作（含 IPC 调用与看门狗），
// 依赖通过 deps 注入（set/get）而非直接引用 store，避免循环依赖并便于单测。
//
// 消息构造已拆到 ./messages.ts、会话生命周期兜底已拆到 ./streamLifecycle.ts
//（原单文件 164 行，逼近 .ts 警告阈值 150）。

import { chatStream } from '@/lib/ipc/chatIpc';
import { flushThrottledStorage } from '@/lib/throttledStorage';
import { dispatchChatEvent } from './events';
import { buildAssistantMessage, buildUserMessage } from './messages';
import { buildRequest } from './request';
import { acceptEvent, resetActiveSeq, setActiveSeq, stopWatchdog } from './streamControl';
import { armWatchdog, sessionReset } from './streamLifecycle';
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

      set((state) => ({
        messages: [...state.messages, buildUserMessage(trimmed)],
        isStreaming: true,
        status: 'searching',
        error: null,
        ...sessionReset(),
      }));

      // FE-C2：invoke 返回前 activeSeq 置空——旧流残余事件（seq 为旧值）
      // 在此期间到达一律丢弃；首帧可能先于 invoke 返回，由收编逻辑处理
      resetActiveSeq();
      armWatchdog({ set, get });

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
      armWatchdog({ set, get });

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
      const assistantMessage = buildAssistantMessage({
        content: currentStream,
        citations: pendingCitations,
        retries,
        lowConfidence: meta.lowConfidence,
      });
      set((state) => ({
        messages: [...state.messages, assistantMessage],
        isStreaming: false,
        status: 'idle',
        ...sessionReset(),
      }));
      // 终态消息立即落盘，不等节流窗口
      flushThrottledStorage();
    },
  };
}
