// 文件管理主页：虚拟滚动列表 + 筛选排序 + 预览抽屉（设计稿 §4 / T6.4）。
//
// 筛选/排序为页面级 state（不污染 store）；选中与文件数据走 fileStore。

import { useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { FileListTable } from '@/components/file/FileListTable';
import { FilePreviewDrawer } from '@/components/file/FilePreviewDrawer';
import {
  filterFiles,
  sortFiles,
  type FileStatus,
  type SortDir,
  type SortKey,
} from '@/lib/fileTable';
import { useFileStore } from '@/stores/fileStore';
import type { FileInfo } from '@/types/ipc';

interface SortState {
  key: SortKey;
  dir: SortDir;
}

export function FilesPage() {
  const files = useFileStore((s) => s.files);
  const scanPath = useFileStore((s) => s.scanPath);
  const isScanning = useFileStore((s) => s.isScanning);
  const selectedIds = useFileStore((s) => s.selectedIds);
  const error = useFileStore((s) => s.error);
  const scanFiles = useFileStore((s) => s.scanFiles);
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);
  const toggleSelect = useFileStore((s) => s.toggleSelect);
  const setSelection = useFileStore((s) => s.setSelection);
  const clearSelection = useFileStore((s) => s.clearSelection);
  const clearError = useFileStore((s) => s.clearError);

  const [categoryFilter, setCategoryFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<'' | FileStatus>('');
  const [sort, setSort] = useState<SortState>({ key: 'name', dir: 'asc' });
  const [previewFile, setPreviewFile] = useState<FileInfo | null>(null);

  const categories = useMemo(() => {
    const set = new Set<string>();
    for (const file of files) {
      if (file.category) {
        set.add(file.category);
      }
    }
    return Array.from(set).sort();
  }, [files]);

  const visibleFiles = useMemo(() => {
    const filtered = filterFiles(files, {
      category: categoryFilter || null,
      status: statusFilter || null,
    });
    return sortFiles(filtered, sort.key, sort.dir);
  }, [files, categoryFilter, statusFilter, sort]);

  const handleScan = async () => {
    const selected = await open({ directory: true, multiple: false, title: '选择要管理的目录' });
    if (typeof selected === 'string') {
      await scanFiles(selected);
    }
  };

  const handleSortChange = (key: SortKey) => {
    setSort((prev) => ({
      key,
      dir: prev.key === key ? (prev.dir === 'asc' ? 'desc' : 'asc') : 'asc',
    }));
  };

  const handleSelectAll = (ids: string[] | null) => {
    if (ids === null) {
      clearSelection();
    } else {
      setSelection(ids);
    }
  };

  return (
    <div className="files-page">
      <header className="files-page__header">
        <h1 className="files-page__title">文件管理</h1>
        <span className="files-page__count">{files.length} 个文件</span>
        {scanPath && (
          <span className="files-page__path" title={scanPath}>
            {scanPath}
          </span>
        )}
      </header>

      {error && (
        <div className="files-page__error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="files-page__error-dismiss"
            aria-label="关闭错误提示"
            onClick={clearError}
          >
            ×
          </button>
        </div>
      )}

      <div className="files-toolbar">
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => void handleScan()}
          disabled={isScanning}
        >
          {isScanning ? '扫描中…' : '扫描目录'}
        </button>
        <button type="button" className="btn btn--ghost" onClick={() => void loadAllFiles()}>
          刷新
        </button>
        <button type="button" className="btn" disabled>
          整理选中 ({selectedIds.length})
        </button>

        <span className="files-toolbar__spacer" />

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
      </div>

      <div className="files-page__body">
        {files.length === 0 ? (
          <div className="files-page__empty">
            <p className="files-page__empty-title">
              {isScanning ? '正在扫描…' : scanPath ? '当前目录暂无文件' : '尚未扫描目录'}
            </p>
            {!isScanning && (
              <p className="files-page__empty-sub">
                {scanPath ? '点击「扫描目录」重新扫描' : '点击「扫描目录」选择要管理的文件夹'}
              </p>
            )}
          </div>
        ) : (
          <div className="files-page__table">
            <FileListTable
              files={visibleFiles}
              selectedIds={selectedIds}
              onToggleSelect={toggleSelect}
              onSelectAll={handleSelectAll}
              onOpenPreview={setPreviewFile}
              sort={sort}
              onSortChange={handleSortChange}
            />
          </div>
        )}
        <FilePreviewDrawer
          key={previewFile?.id}
          file={previewFile}
          onClose={() => setPreviewFile(null)}
        />
      </div>
    </div>
  );
}
