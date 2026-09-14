// 文件页工具栏：整理选中 / 清除选中 / 删除选中 + 搜索框 + 分类/状态筛选。
//
// props 驱动（筛选状态在页面持有）：组件只负责渲染与回调分发，不直接读写 store，
// 便于单独测试与复用。工具栏顺序对齐交互原型：整理选中 → 搜索框 → 筛选。

import type { FileStatus } from '@/lib/fileTable';
import type { FilesPageFilters } from '@/hooks/useFilesPageFilters';

interface FileToolbarProps {
  /** 当前选中的文件数（决定整理/清除/删除三个按钮的可用性与文案） */
  selectedCount: number;
  /** 筛选/搜索状态与回调（useFilesPageFilters 的返回值） */
  filters: FilesPageFilters;
  /** 「整理选中」：重置分类页并跳转 */
  onClassifySelected: () => void;
  /** 「清除选中」：清空选中并作废分类页旧预览 */
  onClearSelection: () => void;
  /** 「删除选中」：打开删除确认弹窗 */
  onRequestDelete: () => void;
}

export function FileToolbar({
  selectedCount,
  filters,
  onClassifySelected,
  onClearSelection,
  onRequestDelete,
}: FileToolbarProps) {
  const {
    categories,
    categoryFilter,
    setCategoryFilter,
    statusFilter,
    setStatusFilter,
    searchQuery,
    setSearchQuery,
  } = filters;

  return (
    <div className="file-toolbar">
      {/* 工具栏顺序对齐文档：整理选中 → 搜索框 → 筛选 */}
      <button
        type="button"
        className="btn btn--sm"
        onClick={onClassifySelected}
        disabled={selectedCount === 0}
      >
        整理选中 ({selectedCount})
      </button>
      {selectedCount > 0 && (
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={onClearSelection}
          data-testid="files-clear-selection"
        >
          清除选中
        </button>
      )}
      {selectedCount > 0 && (
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          style={{ color: 'var(--warn)' }}
          onClick={onRequestDelete}
          data-testid="files-delete-selected"
        >
          删除选中 ({selectedCount})
        </button>
      )}
      <div className="search-box">
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <circle cx="11" cy="11" r="8" />
          <path d="m21 21-4.3-4.3" />
        </svg>
        <input
          type="text"
          placeholder="搜索文件名..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />
      </div>
      <label className="filter-field">
        <span>分类</span>
        <select
          className="files-toolbar__select"
          aria-label="按分类筛选"
          value={categoryFilter}
          onChange={(e) => setCategoryFilter(e.target.value)}
        >
          <option value="">全部分类</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label className="filter-field">
        <span>状态</span>
        <select
          className="files-toolbar__select"
          aria-label="按状态筛选"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value as '' | FileStatus)}
        >
          <option value="">全部状态</option>
          <option value="categorized">已分类</option>
          <option value="uncategorized">未分类</option>
        </select>
      </label>
    </div>
  );
}
