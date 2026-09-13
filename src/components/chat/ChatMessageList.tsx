// 聊天消息区：历史气泡列表 + 流式气泡 + 自动滚底。
//
// 订阅拆分：本组件只订阅 messages（低频，每条消息变一次）；token 级高频
// state（currentStream 等）由内部 StreamingBubble 独立订阅——流式输出时
// 仅重渲流式气泡自身，历史气泡经 React.memo(ChatBubble) 完全跳过。
//
// 自动滚底：仅当用户已贴近底部（< 80px）时跟随，避免打断上翻历史；
// 滚动用 scrollTop 直接赋值（auto）——平滑滚动在高频触发下会堆积卡顿。

import { useEffect, useRef, type RefObject } from 'react';
import { ChatBubble } from './ChatBubble';
import { useChatStore } from '@/stores/chatStore';
import { ChatRole, type ChatCitation, type ChatMessage } from '@/types/models';

interface ChatMessageListProps {
  onCitationClick: (citation: ChatCitation) => void;
}

/** 距底部小于该值视为「贴近底部」，允许自动滚底。 */
const NEAR_BOTTOM_PX = 80;

function isNearBottom(container: HTMLDivElement | null): boolean {
  if (!container) return true;
  return container.scrollHeight - container.scrollTop - container.clientHeight < NEAR_BOTTOM_PX;
}

function scrollToBottom(container: HTMLDivElement | null): void {
  if (!container) return;
  container.scrollTop = container.scrollHeight;
}

export function ChatMessageList({ onCitationClick }: ChatMessageListProps) {
  const messages = useChatStore((s) => s.messages);
  const containerRef = useRef<HTMLDivElement>(null);
  const messagesCount = messages.length;

  useEffect(() => {
    if (isNearBottom(containerRef.current)) {
      scrollToBottom(containerRef.current);
    }
  }, [messagesCount]);

  return (
    <div className="chat-messages" ref={containerRef}>
      {messages.map((message) => (
        <ChatBubble key={message.id} message={message} onCitationClick={onCitationClick} />
      ))}
      <StreamingBubble containerRef={containerRef} onCitationClick={onCitationClick} />
    </div>
  );
}

interface StreamingBubbleProps {
  containerRef: RefObject<HTMLDivElement | null>;
  onCitationClick: (citation: ChatCitation) => void;
}

/** 流式气泡：独立订阅 token 级高频 state，避免整列表 / 整页重渲。 */
function StreamingBubble({ containerRef, onCitationClick }: StreamingBubbleProps) {
  const isStreaming = useChatStore((s) => s.isStreaming);
  const currentStream = useChatStore((s) => s.currentStream);
  const pendingCitations = useChatStore((s) => s.pendingCitations);
  const lowConfidence = useChatStore((s) => s.lowConfidence);
  const retries = useChatStore((s) => s.retries);

  useEffect(() => {
    if (isNearBottom(containerRef.current)) {
      scrollToBottom(containerRef.current);
    }
  }, [containerRef, currentStream]);

  if (!isStreaming) return null;

  const message: ChatMessage = {
    id: 'streaming',
    role: ChatRole.Assistant,
    content: currentStream,
    ...(pendingCitations.length > 0 ? { citations: pendingCitations } : {}),
    ...(lowConfidence ? { lowConfidence: true } : {}),
    ...(retries > 0 ? { retries } : {}),
    createdAt: '',
  };
  return <ChatBubble message={message} streaming onCitationClick={onCitationClick} />;
}
