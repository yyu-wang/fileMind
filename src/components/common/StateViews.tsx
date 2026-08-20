// 通用状态视图（T6.10）：空态 / 错误态 / 加载态。
//
// 供各页面与错误边界复用，统一视觉；图标用 emoji 保持轻量。

import type { ReactNode } from 'react';

interface EmptyStateProps {
  /** 主标题 */
  title: string;
  /** 补充说明 */
  description?: string;
  /** 可选操作区（按钮等） */
  action?: ReactNode;
}

/** 空态：无数据时的占位提示。 */
export function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div className="state-view state-view--empty">
      <div className="state-view__icon" aria-hidden>
        📭
      </div>
      <p className="state-view__title">{title}</p>
      {description && <p className="state-view__desc">{description}</p>}
      {action && <div className="state-view__action">{action}</div>}
    </div>
  );
}

interface ErrorStateProps {
  /** 主标题，默认「出错了」 */
  title?: string;
  /** 错误详情 */
  description?: string;
  /** 重试回调（存在则显示「重新加载」按钮） */
  onRetry?: () => void;
}

/** 错误态：错误边界与操作失败时的提示。 */
export function ErrorState({ title = '出错了', description, onRetry }: ErrorStateProps) {
  return (
    <div className="state-view state-view--error" role="alert">
      <div className="state-view__icon" aria-hidden>
        ⚠️
      </div>
      <p className="state-view__title">{title}</p>
      {description && <p className="state-view__desc">{description}</p>}
      {onRetry && (
        <button type="button" className="btn btn--primary state-view__action" onClick={onRetry}>
          重新加载
        </button>
      )}
    </div>
  );
}

interface LoadingStateProps {
  /** 加载文案，默认「加载中…」 */
  text?: string;
}

/** 加载态：转圈 + 文案。 */
export function LoadingState({ text = '加载中…' }: LoadingStateProps) {
  return (
    <div className="state-view state-view--loading" role="status">
      <div className="state-view__spinner" aria-hidden />
      <p className="state-view__desc">{text}</p>
    </div>
  );
}
