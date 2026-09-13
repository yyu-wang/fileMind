// 聊天气泡（设计稿 05_交互原型 §聊天）：msg + msg-avatar(渐变) + msg-bubble + 流式光标。
//
// 结构：
//   <div class="msg user/ai">
//     <div class="msg-avatar"><svg/></div>
//     <div class="msg-bubble">content + citations</div>
//   </div>

import { memo } from 'react';
import { ChatRole, type ChatCitation, type ChatMessage } from '@/types/models';
import { CitationChip } from './CitationChip';

interface ChatBubbleProps {
  message: ChatMessage;
  /** 流式输出中（渲染闪烁光标） */
  streaming?: boolean;
  onCitationClick: (citation: ChatCitation) => void;
}

// 头像图标（24×24 stroke，对齐文档）
const USER_ICON = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
    <circle cx="12" cy="7" r="4" />
  </svg>
);

const AI_ICON = (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3z" />
  </svg>
);

// memo：流式输出期间父列表重渲时，历史气泡 props（message/回调）不变即跳过
export const ChatBubble = memo(function ChatBubble({
  message,
  streaming = false,
  onCitationClick,
}: ChatBubbleProps) {
  const isUser = message.role === ChatRole.User;

  return (
    <div className={`msg ${isUser ? 'user' : 'ai'}`}>
      <div className="msg-avatar">{isUser ? USER_ICON : AI_ICON}</div>
      <div className="msg-bubble">
        {message.content}
        {streaming && <span className="streaming-cursor" aria-hidden="true" />}

        {message.citations != null && message.citations.length > 0 && (
          <div className="citations">
            <div className="citations-label">📌 引用来源</div>
            {message.citations.map((citation, idx) => (
              // FE-m10：同来源多页引用 id 相同会导致 React key 冲突
              <CitationChip
                key={`${citation.id}-${citation.page}-${idx}`}
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
    </div>
  );
});
