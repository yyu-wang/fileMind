// 聊天输入框：Enter 发送 / Shift+Enter 换行，流式中禁用。

import { useState } from 'react';

interface ChatInputProps {
  disabled: boolean;
  onSend: (content: string) => void;
}

export function ChatInput({ disabled, onSend }: ChatInputProps) {
  const [value, setValue] = useState('');

  const handleSubmit = () => {
    const trimmed = value.trim();
    if (!trimmed || disabled) {
      return;
    }
    onSend(trimmed);
    setValue('');
  };

  return (
    <div className="chat-input">
      <textarea
        className="chat-input__field"
        value={value}
        rows={1}
        placeholder="输入问题，Enter 发送，Shift+Enter 换行"
        aria-label="问题输入框"
        disabled={disabled}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSubmit();
          }
        }}
      />
      <button
        type="button"
        className="btn btn--primary chat-input__send"
        disabled={disabled || value.trim() === ''}
        onClick={handleSubmit}
      >
        发送
      </button>
    </div>
  );
}
