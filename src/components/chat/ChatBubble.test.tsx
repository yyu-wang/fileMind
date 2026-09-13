// ChatBubble 单元测试：角色样式、流式光标、引用标签、低置信度与重试提示。

import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ChatRole, type ChatCitation, type ChatMessage } from '@/types/models';

import { ChatBubble } from './ChatBubble';

function makeMessage(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: 'm1',
    role: ChatRole.Assistant,
    content: 'hello',
    createdAt: '2026-08-01 00:00:00',
    ...overrides,
  };
}

function makeCitation(overrides: Partial<ChatCitation> = {}): ChatCitation {
  return { id: 7, fileName: 'a.pdf', page: 3, text: 'cited', ...overrides };
}

describe('ChatBubble', () => {
  it('applies user/assistant role classes', () => {
    const { container, rerender } = render(
      <ChatBubble message={makeMessage({ role: ChatRole.User })} onCitationClick={vi.fn()} />,
    );
    expect(container.querySelector('.msg.user')).not.toBeNull();
    rerender(<ChatBubble message={makeMessage()} onCitationClick={vi.fn()} />);
    expect(container.querySelector('.msg.ai')).not.toBeNull();
  });

  it('shows streaming cursor when streaming', () => {
    const { container } = render(
      <ChatBubble message={makeMessage()} streaming onCitationClick={vi.fn()} />,
    );
    expect(container.querySelector('.streaming-cursor')).not.toBeNull();
  });

  it('renders no cursor when not streaming', () => {
    const { container } = render(<ChatBubble message={makeMessage()} onCitationClick={vi.fn()} />);
    expect(container.querySelector('.streaming-cursor')).toBeNull();
  });

  it('renders citation chips and forwards click', async () => {
    const user = userEvent.setup();
    const citation = makeCitation();
    const onCitationClick = vi.fn();
    render(
      <ChatBubble
        message={makeMessage({ citations: [citation] })}
        onCitationClick={onCitationClick}
      />,
    );
    expect(screen.getByText('[7]')).toBeInTheDocument();
    await user.click(screen.getByText('a.pdf'));
    expect(onCitationClick).toHaveBeenCalledWith(citation);
  });

  it('renders low-confidence note', () => {
    render(<ChatBubble message={makeMessage({ lowConfidence: true })} onCitationClick={vi.fn()} />);
    expect(screen.getByRole('note')).toHaveTextContent(/低置信度/);
  });

  it('renders self-correction retry count', () => {
    render(<ChatBubble message={makeMessage({ retries: 2 })} onCitationClick={vi.fn()} />);
    expect(screen.getByText(/已自我纠正 2 次/)).toBeInTheDocument();
  });
});
