// chatStore 单元测试：用合成 SSE 事件序列驱动 store（token 追加 → done 组装、
// retry 清缓冲重推、低置信度标记、error 置错、history 取最近 3 轮、请求构造）。
//
// FE-C2/FE-C3 新增：seq 过滤（旧流残余/清空后事件丢弃、首帧先到收编）、
// 流空闲/总量超时看门狗复位。

import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc/chatIpc', () => ({
  chatStream: vi.fn(),
  listenChatEvent: vi.fn(),
}));

import {
  chatStream,
  listenChatEvent,
  type ChatEventData,
  type ChatEvent,
} from '../lib/ipc/chatIpc';
import { ChatRole, type ChatMessage } from '../types/models';
import { useChatStore } from './chatStore';
import { useSettingsStore } from './settingsStore';

function freshState(): void {
  useChatStore.setState({
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
  });
}

function seedHistory(rounds: number): void {
  const messages: ChatMessage[] = [];
  for (let i = 1; i <= rounds; i++) {
    messages.push({ id: `u${i}`, role: ChatRole.User, content: `q${i}`, createdAt: '' });
    messages.push({ id: `a${i}`, role: ChatRole.Assistant, content: `a${i}`, createdAt: '' });
  }
  useChatStore.setState({ messages });
}

/** 发送消息，chatStream mock 返回 seq（默认 1）。 */
async function send(content: string, seq = 1): Promise<void> {
  (chatStream as Mock).mockResolvedValue(seq);
  await useChatStore.getState().sendMessage(content);
}

/** 构造带 request_seq 的合成事件（入参为无 seq 的基础帧）。 */
function ev(e: ChatEventData, seq = 1): ChatEvent {
  return { ...e, request_seq: seq };
}

describe('chatStore 流式渲染', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    freshState();
    vi.clearAllMocks();
    useSettingsStore.setState({
      embeddingModel: 'bge-large-zh-v1.5',
      inferenceMode: 'Local',
      llmModel: 'qwen3.8-27b',
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('token 追加 → citation 暂存 → done 组装含引用的 assistant 消息', async () => {
    await send('FileMind 支持哪些推理模式？');
    const store = useChatStore.getState();
    expect(store.isStreaming).toBe(true);
    expect(store.status).toBe('searching');
    expect(store.messages).toHaveLength(1);
    expect(store.messages[0].role).toBe(ChatRole.User);

    store.handleChatEvent(
      ev({
        event: 'search_start',
        data: {
          query_original: 'FileMind 支持哪些推理模式？',
          query_rewritten: 'FileMind 推理模式',
        },
      }),
    );
    store.handleChatEvent(
      ev({
        event: 'search_result',
        data: {
          candidates: 2,
          after_rerank: 2,
          sources: [
            { id: 1, file_name: '使用说明.md', page: 2, score: 0.92 },
            { id: 2, file_name: '使用说明.md', page: 3, score: 0.88 },
          ],
        },
      }),
    );
    store.handleChatEvent(ev({ event: 'token', data: { content: 'FileMind 支持' } }));
    store.handleChatEvent(ev({ event: 'token', data: { content: '本地推理和云端推理。' } }));
    store.handleChatEvent(
      ev({
        event: 'citation',
        data: {
          citations: [
            { id: 1, file_name: '使用说明.md', page: 2, text: '本地推理使用本机 Ollama 模型' },
          ],
        },
      }),
    );
    store.handleChatEvent(
      ev({
        event: 'done',
        data: { session_id: 's1', total_tokens: 5, duration_ms: 100 },
      }),
    );

    const after = useChatStore.getState();
    expect(after.isStreaming).toBe(false);
    expect(after.status).toBe('idle');
    expect(after.rewrittenQuery).toBeNull();
    expect(after.searchInfo).toBeNull();
    expect(after.messages).toHaveLength(2);
    const assistant = after.messages[1];
    expect(assistant.role).toBe(ChatRole.Assistant);
    expect(assistant.content).toBe('FileMind 支持本地推理和云端推理。');
    expect(assistant.citations).toEqual([
      { id: 1, fileName: '使用说明.md', page: 2, text: '本地推理使用本机 Ollama 模型' },
    ]);
    expect(assistant.lowConfidence).toBeUndefined();
    expect(assistant.retries).toBeUndefined();
  });

  it('search_start/search_result 更新检索状态栏数据', async () => {
    await send('营收多少？');
    const store = useChatStore.getState();
    store.handleChatEvent(
      ev({
        event: 'search_start',
        data: { query_original: '营收多少？', query_rewritten: '2024年Q3营收多少' },
      }),
    );
    expect(useChatStore.getState().rewrittenQuery).toBe('2024年Q3营收多少');

    store.handleChatEvent(
      ev({
        event: 'search_result',
        data: {
          candidates: 2,
          after_rerank: 1,
          sources: [{ id: 1, file_name: '财务报告.pdf', page: 3, score: 0.92 }],
        },
      }),
    );
    const info = useChatStore.getState().searchInfo;
    expect(info).toEqual({
      candidates: 2,
      afterRerank: 1,
      sources: [{ id: 1, fileName: '财务报告.pdf', page: 3, score: 0.92 }],
    });
  });

  it('retry 清空当前缓冲并记录次数，重推 token 后组装带 retries 的消息', async () => {
    await send('营收多少？');
    useChatStore
      .getState()
      .handleChatEvent(ev({ event: 'token', data: { content: '营收为5.5亿元' } }));
    expect(useChatStore.getState().currentStream).toBe('营收为5.5亿元');

    useChatStore.getState().handleChatEvent(
      ev({
        event: 'retry',
        data: {
          reason: '数据错误：营收应为5.2亿',
          attempt: 1,
          rewritten_query: '2024年Q3营收多少',
        },
      }),
    );
    expect(useChatStore.getState().currentStream).toBe('');
    expect(useChatStore.getState().retries).toBe(1);
    expect(useChatStore.getState().retryReason).toBe('数据错误：营收应为5.2亿');

    useChatStore
      .getState()
      .handleChatEvent(ev({ event: 'token', data: { content: '营收为5.2亿元' } }));
    useChatStore.getState().handleChatEvent(
      ev({
        event: 'done',
        data: { session_id: 's1', total_tokens: 2, duration_ms: 50 },
      }),
    );

    const assistant = useChatStore.getState().messages[1];
    expect(assistant.content).toBe('营收为5.2亿元');
    expect(assistant.retries).toBe(1);
    expect(assistant.lowConfidence).toBeUndefined();
  });

  it('done.low_confidence 标记消息低置信度', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '答案' } }));
    useChatStore.getState().handleChatEvent(
      ev({
        event: 'done',
        data: { session_id: 's1', total_tokens: 1, duration_ms: 50, low_confidence: true },
      }),
    );
    const assistant = useChatStore.getState().messages[1];
    expect(assistant.lowConfidence).toBe(true);
  });

  it('error 事件置错并停止流式', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent(
      ev({
        event: 'error',
        data: { code: 'OLLAMA_UNAVAILABLE', message: 'Ollama down' },
      }),
    );
    const state = useChatStore.getState();
    expect(state.error).toBe('Ollama down');
    expect(state.isStreaming).toBe(false);
    expect(state.status).toBe('idle');
    expect(state.messages).toHaveLength(1); // 仅 user 消息
  });

  it('sendMessage 构造请求：settings 模型 + 最近 3 轮 history + 恒空 fts_chunks', async () => {
    seedHistory(4);
    await send('q5');
    const [request] = (chatStream as Mock).mock.calls[0];
    expect(request.query).toBe('q5');
    expect(request.embedding_model).toBe('bge-large-zh-v1.5');
    expect(request.table_name).toBe('documents_bge-large-zh-v1.5_v1');
    expect(request.inference_mode).toBe('local');
    expect(request.llm_model).toBe('qwen3.8-27b');
    expect(request.top_k).toBe(20);
    expect(request.rerank_top_k).toBe(5);
    expect(request.max_retries).toBe(2);
    expect(request.fts_chunks).toEqual([]);
    expect(request.history).toEqual([
      { user: 'q2', assistant: 'a2' },
      { user: 'q3', assistant: 'a3' },
      { user: 'q4', assistant: 'a4' },
    ]);
  });

  it('空内容与流式期间的重复发送被忽略', async () => {
    await useChatStore.getState().sendMessage('   ');
    expect(useChatStore.getState().messages).toHaveLength(0);
    expect(chatStream).not.toHaveBeenCalled();

    await send('第一问');
    expect(chatStream).toHaveBeenCalledTimes(1);
    await useChatStore.getState().sendMessage('第二问');
    expect(useChatStore.getState().messages).toHaveLength(1);
    expect(chatStream).toHaveBeenCalledTimes(1);
  });

  it('chatStream invoke 失败 → 置错并停止流式', async () => {
    (chatStream as Mock).mockRejectedValue(new Error('sidecar 未就绪'));
    await useChatStore.getState().sendMessage('问题');
    const state = useChatStore.getState();
    expect(state.error).toBe('sidecar 未就绪');
    expect(state.isStreaming).toBe(false);
    expect(state.status).toBe('idle');
  });

  it('initChatListener 幂等：只订阅一次', async () => {
    (listenChatEvent as Mock).mockResolvedValue(vi.fn());
    await useChatStore.getState().initChatListener();
    await useChatStore.getState().initChatListener();
    expect(listenChatEvent).toHaveBeenCalledTimes(1);
  });

  it('clearHistory 复位消息与流式状态', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '答' } }));
    useChatStore.getState().clearHistory();
    const state = useChatStore.getState();
    expect(state.messages).toHaveLength(0);
    expect(state.currentStream).toBe('');
    expect(state.isStreaming).toBe(false);
    expect(state.status).toBe('idle');
  });

  // ---------- FE-C2：seq 过滤 ----------

  it('旧流残余 token/error（seq 不匹配）不影响新流内容与状态', async () => {
    await send('第一问', 1);
    // 流 1 完成前用户清空并发起新流（seq=2）
    useChatStore.getState().clearHistory();
    await send('第二问', 2);

    // 旧流（seq=1）迟到的 token 与 error 一律丢弃
    useChatStore
      .getState()
      .handleChatEvent(ev({ event: 'token', data: { content: '旧流残留' } }, 1));
    useChatStore
      .getState()
      .handleChatEvent(ev({ event: 'error', data: { code: 'X', message: '旧流错误' } }, 1));
    const state = useChatStore.getState();
    expect(state.currentStream).toBe('');
    expect(state.error).toBeNull();
    expect(state.isStreaming).toBe(true); // 新流不受影响，仍在等帧

    // 新流（seq=2）正常推进
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '新答案' } }, 2));
    expect(useChatStore.getState().currentStream).toBe('新答案');
  });

  it('首帧先于 invoke 返回到达（seq > lastSeq）被收编，不丢首 token', async () => {
    // 手工制造竞态：invoke 未返回（pending promise）时事件先到
    let resolveInvoke: (seq: number) => void = () => {};
    (chatStream as Mock).mockImplementation(
      () =>
        new Promise((r) => {
          resolveInvoke = r;
        }),
    );
    const pending = useChatStore.getState().sendMessage('问题');
    // invoke 未返回，但首帧已到达（seq=100 > 此前任何流的 lastSeq）→ 收编
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '首' } }, 100));
    expect(useChatStore.getState().currentStream).toBe('首');
    resolveInvoke(100);
    await pending;
  });

  it('clearHistory 后旧流事件全部失效', async () => {
    await send('问题', 5);
    useChatStore.getState().clearHistory();
    // activeSeq 已清 → 旧流（seq=5）事件丢弃
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: 'x' } }, 5));
    const state = useChatStore.getState();
    expect(state.currentStream).toBe('');
    expect(state.isStreaming).toBe(false);
  });

  // ---------- FE-C3：看门狗 ----------

  it('90s 空闲超时：置错误并复位 isStreaming', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '部分' } }));
    // 无新事件推进 90s → 看门狗触发
    vi.advanceTimersByTime(90_001);
    const state = useChatStore.getState();
    expect(state.isStreaming).toBe(false);
    expect(state.status).toBe('idle');
    expect(state.error).toBe('流式响应超时，请重试');
    expect(state.currentStream).toBe('');
  });

  it('done 后看门狗清除：advance 时间不再触发超时', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent(ev({ event: 'token', data: { content: '答' } }));
    useChatStore
      .getState()
      .handleChatEvent(
        ev({ event: 'done', data: { session_id: 's', total_tokens: 1, duration_ms: 1 } }),
      );
    const before = useChatStore.getState().messages;
    vi.advanceTimersByTime(600_001);
    const state = useChatStore.getState();
    expect(state.error).toBeNull(); // 看门狗已清，不再触发
    expect(state.messages).toEqual(before);
    expect(state.isStreaming).toBe(false);
  });
});
