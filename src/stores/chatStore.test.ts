// chatStore 单元测试：用合成 SSE 事件序列驱动 store（token 追加 → done 组装、
// retry 清缓冲重推、低置信度标记、error 置错、history 取最近 3 轮、请求构造）。

import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock('../lib/ipc/chatIpc', () => ({
  chatStream: vi.fn(),
  listenChatEvent: vi.fn(),
}));

import { chatStream, listenChatEvent } from '../lib/ipc/chatIpc';
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

async function send(content: string): Promise<void> {
  (chatStream as Mock).mockResolvedValue(undefined);
  await useChatStore.getState().sendMessage(content);
}

describe('chatStore 流式渲染', () => {
  beforeEach(() => {
    localStorage.clear();
    freshState();
    vi.clearAllMocks();
    useSettingsStore.setState({
      embeddingModel: 'bge-large-zh-v1.5',
      inferenceMode: 'Local',
      llmModel: 'qwen3.8-27b',
    });
  });

  it('token 追加 → citation 暂存 → done 组装含引用的 assistant 消息', async () => {
    await send('FileMind 支持哪些推理模式？');
    const store = useChatStore.getState();
    expect(store.isStreaming).toBe(true);
    expect(store.status).toBe('searching');
    expect(store.messages).toHaveLength(1);
    expect(store.messages[0].role).toBe(ChatRole.User);

    store.handleChatEvent({
      event: 'search_start',
      data: { query_original: 'FileMind 支持哪些推理模式？', query_rewritten: 'FileMind 推理模式' },
    });
    store.handleChatEvent({
      event: 'search_result',
      data: {
        candidates: 2,
        after_rerank: 2,
        sources: [
          { id: 1, file_name: '使用说明.md', page: 2, score: 0.92 },
          { id: 2, file_name: '使用说明.md', page: 3, score: 0.88 },
        ],
      },
    });
    store.handleChatEvent({ event: 'token', data: { content: 'FileMind 支持' } });
    store.handleChatEvent({ event: 'token', data: { content: '本地推理和云端推理。' } });
    store.handleChatEvent({
      event: 'citation',
      data: {
        citations: [
          { id: 1, file_name: '使用说明.md', page: 2, text: '本地推理使用本机 Ollama 模型' },
        ],
      },
    });
    store.handleChatEvent({
      event: 'done',
      data: { session_id: 's1', total_tokens: 5, duration_ms: 100 },
    });

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
    store.handleChatEvent({
      event: 'search_start',
      data: { query_original: '营收多少？', query_rewritten: '2024年Q3营收多少' },
    });
    expect(useChatStore.getState().rewrittenQuery).toBe('2024年Q3营收多少');

    store.handleChatEvent({
      event: 'search_result',
      data: {
        candidates: 2,
        after_rerank: 1,
        sources: [{ id: 1, file_name: '财务报告.pdf', page: 3, score: 0.92 }],
      },
    });
    const info = useChatStore.getState().searchInfo;
    expect(info).toEqual({
      candidates: 2,
      afterRerank: 1,
      sources: [{ id: 1, fileName: '财务报告.pdf', page: 3, score: 0.92 }],
    });
  });

  it('retry 清空当前缓冲并记录次数，重推 token 后组装带 retries 的消息', async () => {
    await send('营收多少？');
    useChatStore.getState().handleChatEvent({ event: 'token', data: { content: '营收为5.5亿元' } });
    expect(useChatStore.getState().currentStream).toBe('营收为5.5亿元');

    useChatStore.getState().handleChatEvent({
      event: 'retry',
      data: { reason: '数据错误：营收应为5.2亿', attempt: 1, rewritten_query: '2024年Q3营收多少' },
    });
    expect(useChatStore.getState().currentStream).toBe('');
    expect(useChatStore.getState().retries).toBe(1);
    expect(useChatStore.getState().retryReason).toBe('数据错误：营收应为5.2亿');

    useChatStore.getState().handleChatEvent({ event: 'token', data: { content: '营收为5.2亿元' } });
    useChatStore.getState().handleChatEvent({
      event: 'done',
      data: { session_id: 's1', total_tokens: 2, duration_ms: 50 },
    });

    const assistant = useChatStore.getState().messages[1];
    expect(assistant.content).toBe('营收为5.2亿元');
    expect(assistant.retries).toBe(1);
    expect(assistant.lowConfidence).toBeUndefined();
  });

  it('done.low_confidence 标记消息低置信度', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent({ event: 'token', data: { content: '答案' } });
    useChatStore.getState().handleChatEvent({
      event: 'done',
      data: { session_id: 's1', total_tokens: 1, duration_ms: 50, low_confidence: true },
    });
    const assistant = useChatStore.getState().messages[1];
    expect(assistant.lowConfidence).toBe(true);
  });

  it('error 事件置错并停止流式', async () => {
    await send('问题');
    useChatStore.getState().handleChatEvent({
      event: 'error',
      data: { code: 'OLLAMA_UNAVAILABLE', message: 'Ollama down' },
    });
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
    useChatStore.getState().handleChatEvent({ event: 'token', data: { content: '答' } });
    useChatStore.getState().clearHistory();
    const state = useChatStore.getState();
    expect(state.messages).toHaveLength(0);
    expect(state.currentStream).toBe('');
    expect(state.isStreaming).toBe(false);
    expect(state.status).toBe('idle');
  });
});
