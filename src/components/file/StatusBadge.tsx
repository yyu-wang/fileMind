// 分类状态徽章：已分类（success）/ 待处理（muted）。
// 文案对齐交互原型 §文件管理 状态列（已分类 / 低置信 / 待处理）。
import type { FileStatus } from '@/lib/fileTable';

interface StatusBadgeProps {
  status: FileStatus;
}

const STATUS_LABELS: Record<FileStatus, string> = {
  categorized: '已分类',
  uncategorized: '待处理',
};

export function StatusBadge({ status }: StatusBadgeProps) {
  return <span className={`badge badge--${status}`}>{STATUS_LABELS[status]}</span>;
}
