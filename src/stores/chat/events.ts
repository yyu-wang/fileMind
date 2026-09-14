// chat://event 事件分发：把后端逐帧事件映射为 store 状态补丁。
//
// 与 store 分离的原因：这是一张「事件 → 状态」的映射表（纯分发，不直接发起 IPC），
// 独立成模块后动作层只负责订阅与 seq 过滤，事件语义集中一处便于对照后端契约。

import type { ChatEvent } from '@/lib/ipc/chatIpc';
import type { ChatCitation } from '@/types/models';
import { resetActiveSeq, stopWatchdog } from './streamControl';
import type { ChatGet, ChatSet } from './types';

/**
 * 按事件类型分发状态更新。
 *
 * Args:
 *   event: chat://event 推送的单帧事件
 *   deps: store 注入的 set/get（`get` 用于回调 appendStreamChunk / finishStream）
 */
export function dispatchChatEvent(event: ChatEvent, deps: { set: ChatSet; get: ChatGet }): void {
  const { set, get } = deps;
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
    case 'citation': {
      // FE-m12：后端已去重，这里再兜底一次（防御 LLM 在 retry 时
      // 产生重复来源 / 未来其他路径引入重复）。按 (id, fileName, page) 三元组去重。
      const seen = new Set<string>();
      const deduped: ChatCitation[] = [];
      for (const c of event.data.citations) {
        const key = `${c.id}|${c.file_name}|${c.page}`;
        if (seen.has(key)) continue;
        seen.add(key);
        deduped.push({ id: c.id, fileName: c.file_name, page: c.page, text: c.text });
      }
      set({ pendingCitations: deduped });
      break;
    }
    case 'done':
      get().finishStream({ lowConfidence: event.data.low_confidence ?? false });
      break;
    case 'error':
      resetActiveSeq();
      stopWatchdog();
      set({
        error: event.data.message,
        isStreaming: false,
        status: 'idle',
        currentStream: '',
      });
      break;
  }
}
