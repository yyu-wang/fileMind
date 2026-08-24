// 分类历史批次明细：逐文件展示文件名、操作类型、状态与 源 → 目标 路径。

import { formatDateTime } from '@/lib/format';
import type { OperationLog } from '@/types/ipc';

import { HistoryStatusTag } from './ClassifyHistoryStatusTag';

const OP_TYPE_LABELS: Record<string, string> = {
  move: '移动',
  rename: '重命名',
  delete: '删除',
};

/** 从绝对路径取文件名（兼容 / 与 Windows \）。 */
function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

interface ClassifyBatchDetailProps {
  logs: OperationLog[];
  onBack: () => void;
}

export function ClassifyBatchDetail({ logs, onBack }: ClassifyBatchDetailProps) {
  return (
    <div className="history-view">
      <div className="history-view__header">
        <div className="history-view__heading">
          <h2 className="history-view__title">批次明细</h2>
          <span className="history-view__sub">
            {formatDateTime(logs[0]?.created_at ?? '')} · {logs.length} 条操作
          </span>
        </div>
        <div className="history-view__actions">
          <button type="button" className="btn btn--ghost" onClick={onBack}>
            返回列表
          </button>
        </div>
      </div>

      <ul className="history-detail">
        {logs.map((log) => (
          <li key={log.id} className="history-detail__item">
            <div className="history-detail__top">
              <span className="history-detail__name" title={log.source_path}>
                {basename(log.source_path)}
              </span>
              <span className="history-detail__op">
                {OP_TYPE_LABELS[log.operation_type] ?? log.operation_type}
              </span>
              <HistoryStatusTag status={log.status} />
            </div>
            <div className="history-detail__paths">
              <span className="history-detail__path" title={log.source_path}>
                {log.source_path}
              </span>
              <span className="history-detail__arrow" aria-hidden>
                →
              </span>
              <span className="history-detail__path" title={log.target_path}>
                {log.target_path}
              </span>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
