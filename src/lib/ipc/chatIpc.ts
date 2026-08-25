// RAG 对话 IPC 封装：chatStream 命令调用 + chat://event 事件订阅。
//
// Rust 端 chat_stream 命令把 Sidecar /chat/stream 的 SSE 帧逐帧经
// `chat://event` 事件推给前端（04_API详细规格书 §3.4）。本文件负责：
// - chatStream(request)：发起流式请求（invoke 立即返回，流在后台推送）
// - listenChatEvent(cb)：订阅 `chat://event`，返回取消订阅函数

import { listen } from '@tauri-apps/api/event';
import { commands, type ChatStreamRequest } from '../../types/ipc';

/** SSE 帧数据，按 event 名判别联合（对齐 Sidecar SSE data 行）。 */
export type ChatEventData =
  | { event: 'search_start'; data: { query_original: string; query_rewritten: string } }
  | {
      event: 'search_result';
      data: {
        candidates: number;
        after_rerank: number;
        sources: Array<{ id: number; file_name: string; page: number; score: number }>;
      };
    }
  | { event: 'token'; data: { content: string } }
  | { event: 'retry'; data: { reason: string; attempt: number; rewritten_query: string } }
  | {
      event: 'citation';
      data: { citations: Array<{ id: number; file_name: string; page: number; text: string }> };
    }
  | {
      event: 'done';
      data: {
        session_id: string;
        total_tokens: number;
        duration_ms: number;
        /** T10.2：检索阶段（改写+向量化+重排）耗时 ms。 */
        retrieve_ms?: number;
        /** T10.2：请求开始到首个 token 的耗时 ms（TTFT）。 */
        first_token_ms?: number;
        low_confidence?: boolean;
      };
    }
  | { event: 'error'; data: { code: string; message: string } };

/**
 * `chat://event` 事件负载 = SSE 帧数据 + `request_seq`。
 *
 * FE-C2：`request_seq` 与 `chatStream` 返回值同源（Rust `request_seq`
 * 计数器），前端按它过滤旧流残余事件（清空对话/新流开始后旧流不再串入）。
 */
export type ChatEvent = ChatEventData & { request_seq: number };

/**
 * 发起 RAG 对话流式请求，返回本次流的请求序号（seq）。
 *
 * 命令立即返回，真正的 SSE 帧由后台任务推送到 `chat://event`，
 * 经 [`listenChatEvent`] 订阅消费。调用失败（如 Sidecar 未就绪）时抛错。
 */
export async function chatStream(request: ChatStreamRequest): Promise<number> {
  const result = await commands.chatStream(request);
  if (result.status !== 'ok') {
    throw new Error(result.error);
  }
  // 成功路径 Rust 恒返回 seq；null 兜底为 0（typedError 类型的名义空值）
  return result.data ?? 0;
}

/**
 * 订阅 `chat://event` 流式事件，返回取消订阅函数。
 *
 * 调用方应在组件卸载时调用返回的 unlisten 清理（避免事件泄漏）。
 */
export async function listenChatEvent(handler: (event: ChatEvent) => void): Promise<() => void> {
  const unlisten = await listen<ChatEvent>('chat://event', (payload) => {
    handler(payload.payload);
  });
  return unlisten;
}
