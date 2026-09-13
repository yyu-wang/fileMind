// 检索状态栏：改写查询、候选/重排数、生成阶段、重试与低置信度提示。
//
// 数据自行从 chatStore 订阅（含 currentStream.length > 0 的 hasTokens 派生），
// 让页面组件不必接触 token 级高频 state——hasTokens 返回 boolean，
// zustand Object.is 比较下仅在「无 token ↔ 有 token」边界触发重渲。

import { useChatStore } from '@/stores/chatStore';

export function SearchStatusBar() {
  const status = useChatStore((s) => s.status);
  const rewrittenQuery = useChatStore((s) => s.rewrittenQuery);
  const searchInfo = useChatStore((s) => s.searchInfo);
  const retries = useChatStore((s) => s.retries);
  const retryReason = useChatStore((s) => s.retryReason);
  const lowConfidence = useChatStore((s) => s.lowConfidence);
  const hasTokens = useChatStore((s) => s.currentStream.length > 0);

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
