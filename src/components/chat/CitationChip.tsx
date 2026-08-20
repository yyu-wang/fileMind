// 引用标签：点击跳转文件预览并定位到对应页码（设计稿 §6 引用来源）。

import type { ChatCitation } from '@/types/models';

interface CitationChipProps {
  citation: ChatCitation;
  onClick: () => void;
}

export function CitationChip({ citation, onClick }: CitationChipProps) {
  return (
    <button
      type="button"
      className="chat-citation"
      onClick={onClick}
      title={`${citation.fileName} 第 ${citation.page} 页`}
    >
      <span className="chat-citation__idx">[{citation.id}]</span>
      <span className="chat-citation__name">{citation.fileName}</span>
      <span className="chat-citation__page">P{citation.page}</span>
    </button>
  );
}
