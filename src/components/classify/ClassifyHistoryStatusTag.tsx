// 批次/日志状态徽标：把后端状态字符串（pending/done/failed/undone）
// 映射为展示文案与配色，列表与明细共用。

const STATUS_META: Record<string, { label: string; className: string }> = {
  done: { label: '成功', className: 'tag--green' },
  failed: { label: '失败', className: 'tag--red' },
  undone: { label: '已撤销', className: 'tag--gray' },
  pending: { label: '待处理', className: 'tag--amber' },
};

interface HistoryStatusTagProps {
  status: string;
}

export function HistoryStatusTag({ status }: HistoryStatusTagProps) {
  const meta = STATUS_META[status] ?? { label: status, className: 'tag--gray' };
  return <span className={`tag ${meta.className}`}>{meta.label}</span>;
}
