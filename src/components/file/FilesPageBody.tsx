// 文件页主体：已扫描目录面板 + 工具栏 + 空态/虚拟滚动表格 + 预览抽屉。
//
// 数据（筛选结果、选中、空态文案、预览目标）与动作都由页面传入，本组件只负责渲染，
// 不直接读写 store；虚拟滚动自带滚动容器，故不用 main-content 的 overflow。

import type { FilesPageFilters } from '@/hooks/useFilesPageFilters';
import type { FilesEmptyCopy } from '@/lib/filesPageEmpty';
import type { FileInfo } from '@/types/ipc';

import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';

import { FileListTable } from './FileListTable';
import { FileToolbar } from './FileToolbar';
import { ScannedDirectoriesPanel } from './ScannedDirectoriesPanel';

interface FilesPageBodyProps {
  /** 列表数据：筛选/排序状态、选中、空态文案、预览目标 */
  list: {
    filters: FilesPageFilters;
    selectedIds: string[];
    emptyCopy: FilesEmptyCopy | null;
    previewFile: FileInfo | null;
  };
  /** 区域动作：面板刷新、表格选中、预览开关、工具栏三键 */
  actions: {
    onRefresh: () => void;
    onToggleSelect: (id: string) => void;
    onSelectAll: (ids: string[] | null) => void;
    onOpenPreview: (file: FileInfo) => void;
    onClosePreview: () => void;
    onClassifySelected: () => void;
    onClearSelection: () => void;
    onRequestDelete: () => void;
  };
}

export function FilesPageBody({ list, actions }: FilesPageBodyProps) {
  return (
    <div className="files-page__body">
      <ScannedDirectoriesPanel onRemoved={actions.onRefresh} />
      <FileToolbar
        selectedCount={list.selectedIds.length}
        filters={list.filters}
        onClassifySelected={actions.onClassifySelected}
        onClearSelection={actions.onClearSelection}
        onRequestDelete={actions.onRequestDelete}
      />

      {list.emptyCopy !== null ? (
        <div className="files-page__empty">
          <p className="files-page__empty-title">{list.emptyCopy.title}</p>
          {list.emptyCopy.sub !== null && (
            <p className="files-page__empty-sub">{list.emptyCopy.sub}</p>
          )}
        </div>
      ) : (
        <div className="files-page__table">
          <FileListTable
            files={list.filters.visibleFiles}
            selectedIds={list.selectedIds}
            onToggleSelect={actions.onToggleSelect}
            onSelectAll={actions.onSelectAll}
            onOpenPreview={actions.onOpenPreview}
            sort={list.filters.sort}
            onSortChange={list.filters.toggleSort}
          />
        </div>
      )}
      <LazyFilePreviewDrawer
        key={list.previewFile?.id}
        file={list.previewFile}
        onClose={actions.onClosePreview}
      />
    </div>
  );
}
