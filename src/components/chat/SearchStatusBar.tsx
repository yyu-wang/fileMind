// 检索状态栏：改写查询、候选/重排数、生成阶段、重试与低置信度提示。

import type { ChatSearchInfo, ChatStatus } from '@/stores/chatStore';

interface SearchStatusBarProps {
  status: ChatStatus;
  rewrittenQuery: string | null;
  searchInfo: ChatSearchInfo | null;
  retries: number;
  retryReason: string | null;
  lowConfidence: boolean;
  /** 是否已有流式 token（区分检索中 / 生成中） */
  hasTokens: boolean;
}

export function SearchStatusBar({
  status,
  rewrittenQuery,
  searchInfo,
  retries,
  retryReason,
  lowConfidence,
  hasTokens,
}: SearchStatusBarProps) {
  if (status === 'idle') {
    return null;
  }

  return (
    <div className="chat-status" role="status" aria-live="polite">
      {searchInfo && (
        <span className="chat-status__item">
          检索到 {searchInfo.candidates} 个候选 · 重排后 {searchInfo.afterRerank} 条
        </span>
      )}
      {rewrittenQuery && <span className="chat-status__item">改写查询: {rewrittenQuery}</span>}
      {status === 'searching' && !hasTokens && <span className="chat-status__item">正在检索…</span>}
      {hasTokens && <span className="chat-status__item">正在生成…</span>}
      {retries > 0 && (
        <span className="chat-status__item chat-status__item--warn">
          自我纠正 {retries} 次{retryReason ? `：${retryReason}` : ''}
        </span>
      )}
      {lowConfidence && <span className="chat-status__item chat-status__item--warn">低置信度</span>}
    </div>
  );
}
