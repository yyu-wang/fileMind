// 文件列表的虚拟行（从 FileListTable 拆出：原文件逼近 .tsx 警告阈值 200）。
//
// memo：FilesPage 任意 state 变化（如搜索每键触发）重渲整表时，
// props（file 引用 / selected / 稳定回调 / top）不变的行整体跳过。

import { memo } from 'react';

import { formatDateTime, formatFileSize, getFileTypeMeta } from '@/lib/format';
import { categoryTagClass, deriveFileStatus, isOrganized } from '@/lib/fileTable';
import type { FileInfo } from '@/types/ipc';

import { StatusBadge } from './StatusBadge';

interface FileRowProps {
  file: FileInfo;
  selected: boolean;
  onToggleSelect: (id: string) => void;
  onOpenPreview: (file: FileInfo) => void;
  /** 虚拟行纵向偏移（px）。传数字而非 style 对象，memo 浅比较才有效 */
  top: number;
}

export const FileRow = memo(function FileRow({
  file,
  selected,
  onToggleSelect,
  onOpenPreview,
  top,
}: FileRowProps) {
  const status = deriveFileStatus(file);
  const typeMeta = getFileTypeMeta(file.file_name);
  // 已整理行弱化样式（标记 + 与状态列「已分类」徽标呼应），软排除提示
  const organizedClass = isOrganized(file) ? ' files-table__row--organized' : '';
  const rowClass = selected
    ? `files-table__row files-table__row--selected${organizedClass}`
    : `files-table__row${organizedClass}`;

  return (
    <div
      className={rowClass}
      style={{ transform: `translateY(${top}px)` }}
      role="row"
      onClick={() => onOpenPreview(file)}
    >
      <div className="files-table__cell files-table__cell--check" role="cell">
        <input
          type="checkbox"
          aria-label={`选择 ${file.file_name}`}
          checked={selected}
          onChange={() => onToggleSelect(file.id)}
          onClick={(e) => e.stopPropagation()}
        />
      </div>
      {/* 类型图标（对齐交互原型彩色块） */}
      <div className="files-table__cell files-table__cell--type" role="cell">
        <span className={`file-type-icon ${typeMeta.kind}`} title={file.file_name}>
          {typeMeta.label}
        </span>
      </div>
      <div className="files-table__cell files-table__cell--name" role="cell" title={file.path}>
        {file.file_name}
      </div>
      <div className="files-table__cell files-table__cell--size" role="cell">
        {formatFileSize(file.file_size)}
      </div>
      <div className="files-table__cell files-table__cell--time" role="cell">
        {formatDateTime(file.updated_at)}
      </div>
      <div className="files-table__cell files-table__cell--cat" role="cell">
        {file.category ? (
          <span className={categoryTagClass(file.category)}>{file.category}</span>
        ) : (
          <span className="tag tag--gray">未分类</span>
        )}
      </div>
      <div className="files-table__cell files-table__cell--status" role="cell">
        <StatusBadge status={status} />
      </div>
    </div>
  );
});
