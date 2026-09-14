// 聊天生命周期动作：清空对话历史 / 清除错误。
//
// 与 store 分离的原因：只做「复位 + 落盘」编排，依赖 store 注入的 set。

import { flushThrottledStorage } from '@/lib/throttledStorage';
import { resetActiveSeq, stopWatchdog } from './streamControl';
import type { ChatSet, ChatState } from './types';

/**
 * 生成生命周期动作集合（供 store 展开进 create 的返回对象）。
 *
 * Args:
 *   deps: store 注入的 set
 *
 * Returns:
 *   clearHistory / clearError 两个动作
 */
export function createLifecycleActions({
  set,
}: {
  set: ChatSet;
}): Pick<ChatState, 'clearHistory' | 'clearError'> {
  return {
    clearHistory: () => {
      // FE-C2：作废在途流——清 seq 后旧流残余事件全部失效，看门狗停止
      resetActiveSeq();
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
      // 清空也立即落盘，避免节流窗口内的旧历史被延迟恢复
      flushThrottledStorage();
    },

    clearError: () => set({ error: null }),
  };
}
