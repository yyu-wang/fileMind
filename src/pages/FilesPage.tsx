// 文件管理主页：虚拟滚动列表 + 筛选排序 + 预览抽屉（设计稿 05_交互原型 §文件管理）。
//
// 结构：main-header(h1 + subtitle + header-actions) → main-content(file-toolbar + file-table)
// 页面只做编排：订阅与派生在 useFilesPageData，动作（扫描/选中/预览/删除）在
// useFilesPageActions，头部与主体各为一个区域组件（components/file/）。

import { PageErrorBanner } from '@/components/common/PageErrorBanner';
import { FilesPageBody } from '@/components/file/FilesPageBody';
import { FilesPageHeader } from '@/components/file/FilesPageHeader';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { useFilesPageActions } from '@/hooks/useFilesPageActions';
import { useFilesPageData } from '@/hooks/useFilesPageData';

export function FilesPage() {
  const data = useFilesPageData();
  const actions = useFilesPageActions();

  return (
    <div className="page files-page">
      <FilesPageHeader
        scanPath={data.scanPath}
        isScanning={data.isScanning}
        onRefresh={actions.refresh}
        onScan={actions.scan}
      />
      <PageErrorBanner
        message={data.error}
        className="files-page__error"
        dismissClassName="files-page__error-dismiss"
        onDismiss={actions.clearError}
      />
      <FilesPageBody
        list={{
          filters: data.filters,
          selectedIds: data.selectedIds,
          emptyCopy: data.emptyCopy,
          previewFile: actions.previewFile,
        }}
        actions={{
          onRefresh: actions.refresh,
          onToggleSelect: actions.toggleSelect,
          onSelectAll: actions.selectAll,
          onOpenPreview: actions.openPreview,
          onClosePreview: actions.closePreview,
          onClassifySelected: actions.classifySelected,
          onClearSelection: actions.clearSelection,
          onRequestDelete: actions.requestDelete,
        }}
      />

      {actions.pendingDelete && (
        <ConfirmDialog
          title="删除选中文件"
          message={`将把选中的 ${data.selectedIds.length} 个文件移入系统回收站（可在系统回收站恢复）；应用内不提供撤销。`}
          confirmLabel="移入回收站"
          danger
          loading={actions.deleting}
          onConfirm={actions.confirmDelete}
          onCancel={actions.cancelDelete}
        />
      )}
    </div>
  );
}
