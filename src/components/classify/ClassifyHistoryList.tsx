// 分类历史列表：批次摘要（时间 / 成功数 / 状态 / 可撤销操作）。
//
// 受控组件：数据与 loading/error 来自 classifyHistoryStore，由 ClassifyPage 传入；
// 撤销只上报意图（onRequestUndo），二次确认对话框在页面层统一处理。

import { formatDateTime } from '@/lib/format';
import type { OperationBatchSummary } from '@/types/ipc';

import { HistoryStatusTag } from './ClassifyHistoryStatusTag';

const OP_TYPE_LABELS: Record<string, string> = {
  move: '移动',
  rename: '重命名',
  delete: '删除',
};

interface ClassifyHistoryListProps {
  batches: OperationBatchSummary[];
  loading: boolean;
  error: string | null;
  undoing: boolean;
  onRefresh: () => void;
  onBack: () => void;
  onOpenBatch: (batchId: string) => void;
  onRequestUndo: (batch: OperationBatchSummary) => void;
}

export function ClassifyHistoryList({
  batches,
  loading,
  error,
  undoing,
  onRefresh,
  onBack,
  onOpenBatch,
  onRequestUndo,
}: ClassifyHistoryListProps) {
  return (
    <div className="history-view">
      <div className="history-view__header">
        <div className="history-view__heading">
          <h2 className="history-view__title">分类历史</h2>
          <span className="history-view__sub">最近 {batches.length} 批整理记录</span>
        </div>
        <div className="history-view__actions">
          <button type="button" className="btn btn--ghost" onClick={onBack}>
            返回
          </button>
          <button type="button" className="btn" onClick={onRefresh} disabled={loading}>
            {loading ? '刷新中…' : '刷新'}
          </button>
        </div>
      </div>

      {error && (
        <div className="history-view__error" role="alert">
          {error}
        </div>
      )}

      {loading && batches.length === 0 ? (
        <div className="history-view__empty" role="status">
          正在加载分类历史…
        </div>
      ) : batches.length === 0 ? (
        <div className="history-view__empty">
          <p className="history-view__empty-title">暂无分类历史</p>
          <p className="history-view__empty-sub">在智能分类中执行整理后，这里会显示批次记录。</p>
        </div>
      ) : (
        <ul className="history-list">
          {batches.map((batch) => (
            <li key={batch.batch_id} className="history-item">
              <button
                type="button"
                className="history-item__main"
                onClick={() => onOpenBatch(batch.batch_id)}
              >
                <span className="history-item__time">{formatDateTime(batch.created_at)}</span>
                <span className="history-item__meta">
                  {OP_TYPE_LABELS[batch.op_type] ?? batch.op_type} · {batch.success_count}/
                  {batch.total_count} 成功
                </span>
                <HistoryStatusTag status={batch.status} />
              </button>
              {batch.can_undo && (
                <button
                  type="button"
                  className="btn btn--ghost history-item__undo"
                  onClick={() => onRequestUndo(batch)}
                  disabled={undoing}
                >
                  撤销
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
