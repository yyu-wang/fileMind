// 聊天气泡：user / assistant 气泡 + 流式光标 + 引用标签 + 低置信度标记。

import { ChatRole, type ChatCitation, type ChatMessage } from '@/types/models';
import { CitationChip } from './CitationChip';

interface ChatBubbleProps {
  message: ChatMessage;
  /** 流式输出中（渲染闪烁光标） */
  streaming?: boolean;
  onCitationClick: (citation: ChatCitation) => void;
}

export function ChatBubble({ message, streaming = false, onCitationClick }: ChatBubbleProps) {
  const isUser = message.role === ChatRole.User;

  return (
    <div className={`chat-bubble ${isUser ? 'chat-bubble--user' : 'chat-bubble--assistant'}`}>
      <div className="chat-bubble__content">
        {message.content}
        {streaming && <span className="chat-bubble__cursor" aria-hidden="true" />}
      </div>

      {message.citations != null && message.citations.length > 0 && (
        <div className="chat-bubble__citations">
          {message.citations.map((citation) => (
            <CitationChip
              key={citation.id}
              citation={citation}
              onClick={() => onCitationClick(citation)}
            />
          ))}
        </div>
      )}

      {message.lowConfidence && (
        <div className="chat-bubble__flag" role="note">
          低置信度：答案可能不够准确，请结合引用来源核对
        </div>
      )}
      {message.retries != null && message.retries > 0 && (
        <div className="chat-bubble__flag">已自我纠正 {message.retries} 次</div>
      )}
    </div>
  );
}
