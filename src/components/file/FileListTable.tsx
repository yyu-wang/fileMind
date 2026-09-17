// 文件列表表格：虚拟滚动 + 列头排序 + 多选 + 行点击预览。
//
// props 全部受控：数据、选中、排序状态均由父级（FilesPage）持有，
// 本组件只负责渲染与事件上报，不直接读写 store。
//
// 行渲染已拆到同目录 FileRow.tsx（原单文件 226 行，逼近 .tsx 警告阈值 200）。

import { useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { isOrganized, type SortKey, type SortState } from '@/lib/fileTable';
import type { FileInfo } from '@/types/ipc';

import { FileRow } from './FileRow';

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
