// 文件列表表格：虚拟滚动 + 列头排序 + 多选 + 行点击预览。
//
// props 全部受控：数据、选中、排序状态均由父级（FilesPage）持有，
// 本组件只负责渲染与事件上报，不直接读写 store。

import { memo, useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { formatDateTime, formatFileSize, getFileTypeMeta } from '@/lib/format';
import {
  categoryTagClass,
  deriveFileStatus,
  isOrganized,
  type SortDir,
  type SortKey,
} from '@/lib/fileTable';
import type { FileInfo } from '@/types/ipc';

import { StatusBadge } from './StatusBadge';

interface SortState {
  key: SortKey;
  dir: SortDir;
}

interface FileListTableProps {
  files: FileInfo[];
  selectedIds: string[];
  onToggleSelect: (id: string) => void;
  onSelectAll: (ids: string[] | null) => void;
  onOpenPreview: (file: FileInfo) => void;
  sort: SortState;
  onSortChange: (key: SortKey) => void;
}

const ROW_ESTIMATED_SIZE = 36;
const ROW_OVERSCAN = 10;

const SORTABLE_HEADERS: { key: SortKey; label: string }[] = [
  { key: 'name', label: '文件名' },
  { key: 'size', label: '大小' },
  { key: 'time', label: '修改时间' },
];

export function FileListTable({
  files,
  selectedIds,
  onToggleSelect,
  onSelectAll,
  onOpenPreview,
  sort,
  onSortChange,
}: FileListTableProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const checkboxRef = useRef<HTMLInputElement>(null);
  // react-virtual 返回值不参与 memo，本组件也未对虚拟化结果做缓存，安全
  // eslint-disable-next-line react-hooks/incompatible-library
  const rowVirtualizer = useVirtualizer({
    count: files.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_ESTIMATED_SIZE,
    overscan: ROW_OVERSCAN,
  });

  // 软排除：表头「全选」只圈选未整理文件；已整理文件可手动勾选重分类，但不进批量。
  // T10.4：万级文件下这两个派生量是 O(N)，用 useMemo 缓存避免每次渲染重算
  //（重排/勾选改变才重算，与父级持有的受控状态变更频率一致）。
  const selectableIds = useMemo(
    () => files.filter((file) => !isOrganized(file)).map((file) => file.id),
    [files],
  );
  // FE-M8：数组 includes 是 O(M)，万级文件全选时表头计数 + 每行选中判断都是 O(N×M)；
  // Set 化后成员判断 O(1)。依赖 selectedIds 引用变化重算（父级每次勾选新建数组）。
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const selectedSelectableCount = useMemo(
    () => selectableIds.filter((id) => selectedIdSet.has(id)).length,
    [selectableIds, selectedIdSet],
  );
  const allSelectableSelected =
    selectableIds.length > 0 && selectedSelectableCount === selectableIds.length;
  const someSelectableSelected = selectedSelectableCount > 0;

  useEffect(() => {
    if (checkboxRef.current) {
      checkboxRef.current.indeterminate = someSelectableSelected && !allSelectableSelected;
    }
  }, [someSelectableSelected, allSelectableSelected]);

  const handleSelectAll = () => {
    onSelectAll(allSelectableSelected ? null : selectableIds);
  };

  const renderHeaderArrow = (key: SortKey) => (
    <span className="files-table__sort" aria-hidden>
      {sort.key === key ? (sort.dir === 'asc' ? '↑' : '↓') : '↕'}
    </span>
  );

  const ariaSortFor = (key: SortKey) =>
    sort.key === key ? (sort.dir === 'asc' ? 'ascending' : 'descending') : undefined;

  return (
    <div className="files-table">
      <div className="files-table__head" role="row">
        <div className="files-table__cell files-table__cell--check" role="columnheader">
          <input
            ref={checkboxRef}
            type="checkbox"
            aria-label="全选当前列表"
            checked={allSelectableSelected}
            onChange={handleSelectAll}
          />
        </div>
        {/* 类型图标列（对齐交互原型，无表头文字） */}
        <div className="files-table__cell files-table__cell--type" role="columnheader" />
        {SORTABLE_HEADERS.map(({ key, label }) => (
          <div
            key={key}
            className="files-table__cell files-table__cell--sortable"
            role="columnheader"
            aria-sort={ariaSortFor(key)}
          >
            <button
              type="button"
              className="files-table__sortbtn"
              onClick={() => onSortChange(key)}
            >
              {label}
              {renderHeaderArrow(key)}
            </button>
          </div>
        ))}
        <div className="files-table__cell files-table__cell--cat" role="columnheader">
          分类
        </div>
        <div className="files-table__cell files-table__cell--status" role="columnheader">
          状态
        </div>
      </div>

      <div className="files-table__body" ref={parentRef}>
        <div className="files-table__canvas" style={{ height: rowVirtualizer.getTotalSize() }}>
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const file = files[virtualRow.index];
            return (
              <FileRow
                key={file.id}
                file={file}
                selected={selectedIdSet.has(file.id)}
                onToggleSelect={onToggleSelect}
                onOpenPreview={onOpenPreview}
                top={virtualRow.start}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

interface FileRowProps {
  file: FileInfo;
  selected: boolean;
  onToggleSelect: (id: string) => void;
  onOpenPreview: (file: FileInfo) => void;
  /** 虚拟行纵向偏移（px）。传数字而非 style 对象，memo 浅比较才有效 */
  top: number;
}

// memo：FilesPage 任意 state 变化（如搜索每键触发）重渲本表时，
// props（file 引用 / selected / 稳定回调 / top）不变的行整体跳过
const FileRow = memo(function FileRow({
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
