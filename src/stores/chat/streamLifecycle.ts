// 流式会话的生命周期兜底：会话级临时态归零 + 看门狗武装。
//
// 从 streaming.ts 拆出（该文件逼近 .ts 警告阈值 150）。与 streamControl.ts 的分工：
// 那边是「非响应式」的 seq/计时器原语、刻意不碰 store；这边是「流式状态语义」，
// 需要 set/get 才能复位，故单独成模块。

import { resetActiveSeq, restartWatchdog, stopWatchdog } from './streamControl';
import type { ChatGet, ChatSet, ChatState } from './types';

/**
 * 一轮问答的会话级临时态归零。
 *
 * 新提问起点（sendMessage）与终态收尾（finishStream）、超时兜底三处都必须复位同一组
 * 字段，**必须同步**：漏一个就会出现「新提问带着上一轮的引用/改写结果」。
 */
export function sessionReset(): Partial<ChatState> {
  return {
    currentStream: '',
    pendingCitations: [],
    rewrittenQuery: null,
    searchInfo: null,
    retries: 0,
    retryReason: null,
    lowConfidence: false,
  };
}

/**
 * FE-C3：流级看门狗超时兜底——复位为错误态并清 seq 与看门狗。
 *
 * 发送动作与事件入口都要重新计时，回调体相同故共用（done/error/clearHistory 时清除）。
 */
export function armWatchdog(deps: { set: ChatSet; get: ChatGet }): void {
  restartWatchdog(() => {
    const s = deps.get();
    if (!s.isStreaming) return;
    stopWatchdog();
    resetActiveSeq();
    deps.set({
      error: '流式响应超时，请重试',
      isStreaming: false,
      status: 'idle',
      ...sessionReset(),
    });
  });
}
