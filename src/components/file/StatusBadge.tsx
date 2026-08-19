// 分类状态徽章：已分类（success）/ 未分类（muted）。
import type { FileStatus } from '@/lib/fileTable';

interface StatusBadgeProps {
  status: FileStatus;
}

const STATUS_LABELS: Record<FileStatus, string> = {
  categorized: '已分类',
  uncategorized: '未分类',
};

export function StatusBadge({ status }: StatusBadgeProps) {
  return <span className={`badge badge--${status}`}>{STATUS_LABELS[status]}</span>;
}
