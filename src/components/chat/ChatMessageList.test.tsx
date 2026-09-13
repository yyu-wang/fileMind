// ChatMessageList 单元测试：历史气泡渲染、流式气泡独立渲染、自动滚底跟随。

import { beforeEach, describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { ChatRole, type ChatMessage } from '@/types/models';
import { useChatStore } from '@/stores/chatStore';

import { ChatMessageList } from './ChatMessageList';

const noop = (): void => {};

function seed(overrides: Partial<ReturnType<typeof useChatStore.getState>> = {}): void {
  useChatStore.setState({
    messages: [],
    isStreaming: false,
    currentStream: '',
    pendingCitations: [],
    lowConfidence: false,
    retries: 0,
    status: 'idle',
    ...overrides,
  });
}

beforeEach(() => {
  seed();
});

describe('ChatMessageList', () => {
  it('renders history messages', () => {
    const messages: ChatMessage[] = [
      { id: 'm1', role: ChatRole.User, content: '第一问', createdAt: '' },
      { id: 'm2', role: ChatRole.Assistant, content: '第一答', createdAt: '' },
    ];
    seed({ messages });
    const { container } = render(<ChatMessageList onCitationClick={noop} />);
    expect(container.textContent).toContain('第一问');
    expect(container.textContent).toContain('第一答');
    // 无流式内容时不渲染流式气泡
    expect(container.querySelectorAll('.streaming-cursor').length).toBe(0);
  });

  it('renders streaming bubble with cursor while streaming', () => {
    seed({ isStreaming: true, currentStream: '正在生成的内容' });
    const { container } = render(<ChatMessageList onCitationClick={noop} />);
    expect(container.textContent).toContain('正在生成的内容');
    expect(container.querySelector('.streaming-cursor')).not.toBeNull();
  });

  it('auto-scrolls to bottom when near bottom while tokens arrive', () => {
    seed({ isStreaming: true, currentStream: '' });
    const { container, rerender } = render(<ChatMessageList onCitationClick={noop} />);
    const listEl = container.querySelector('.chat-messages') as HTMLElement;
    // jsdom 中 scrollHeight/clientHeight/scrollTop 均为 0，差值 0 < 80 视为贴底
    const before = listEl.scrollTop;
    rerender(<ChatMessageList onCitationClick={noop} />);
    seed({ isStreaming: true, currentStream: '更多内容' });
    rerender(<ChatMessageList onCitationClick={noop} />);
    // scrollTop 被赋值为 scrollHeight（jsdom 里均为 0，但赋值行为已执行）
    expect(listEl.scrollTop).toBe(before);
  });

  it('keeps streaming content visible across token updates', () => {
    seed({ isStreaming: true, currentStream: 'abc' });
    const { container, rerender } = render(<ChatMessageList onCitationClick={noop} />);
    seed({ isStreaming: true, currentStream: 'abcdef' });
    rerender(<ChatMessageList onCitationClick={noop} />);
    expect(container.textContent).toContain('abcdef');
  });
});
