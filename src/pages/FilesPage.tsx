// 文件管理主页：虚拟滚动列表 + 筛选排序 + 预览抽屉（设计稿 §4 / T6.4）。
//
// 筛选/排序为页面级 state（不污染 store）；选中与文件数据走 fileStore。

import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { open } from '@tauri-apps/plugin-dialog';

import { FileListTable } from '@/components/file/FileListTable';
import { FilePreviewDrawer } from '@/components/common/FilePreviewDrawer';
import { useHotkeys } from '@/hooks/useHotkeys';
import { getE2eTestDir } from '@/lib/e2e';
import {
  filterByName,
  filterFiles,
  sortFiles,
  type FileStatus,
  type SortDir,
  type SortKey,
} from '@/lib/fileTable';
import { useClassifyStore } from '@/stores/classifyStore';
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
  const navigate = useNavigate();

  const [categoryFilter, setCategoryFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<'' | FileStatus>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [sort, setSort] = useState<SortState>({ key: 'name', dir: 'asc' });
  const [previewFile, setPreviewFile] = useState<FileInfo | null>(null);

  // T6.10 快捷键：Space 预览选中的第一个文件（无修饰键，输入框内自动跳过）
  useHotkeys([
    {
      key: ' ',
      handler: () => {
        // FE-M8：find 内逐个 includes 是 O(N×M)，Set 化后整体 O(N+M)
        const selectedSet = new Set(selectedIds);
        const first = files.find((f) => selectedSet.has(f.id));
        if (first) setPreviewFile(first);
      },
    },
  ]);

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
    // 客户端搜索（对齐交互原型搜索框，数据全量在内存）
    return sortFiles(filterByName(filtered, searchQuery), sort.key, sort.dir);
  }, [files, categoryFilter, statusFilter, searchQuery, sort]);

  // T9.5 E2E：测试目录存在时跳过原生对话框（原生 open() 无法被 WebDriver 点击）。
  const handleScan = async () => {
    const testDir = await getE2eTestDir();
    if (testDir) {
      await scanFiles(testDir);
      return;
    }
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

  const handleClassifySelected = () => {
    useClassifyStore.getState().reset();
    navigate('/classify');
  };

  return (
    <div className="files-page">
      <header className="files-page__header">
        <div className="files-page__heading">
          <h1 className="files-page__title">文件管理</h1>
          {scanPath && (
            <span className="files-page__path" title={scanPath}>
              {scanPath}
            </span>
          )}
        </div>
        <div className="files-page__actions">
          {/* FE-C4：刷新也进 isScanning 态（store），狂点被防抖 */}
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => void loadAllFiles()}
            disabled={isScanning}
          >
            {isScanning ? '刷新中…' : '刷新'}
          </button>
          <button
            type="button"
            className="btn btn--primary"
            data-testid="files-scan"
            onClick={() => void handleScan()}
            disabled={isScanning}
          >
            {isScanning ? '扫描中…' : '扫描目录'}
          </button>
        </div>
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
        {/* 搜索框（对齐交互原型） */}
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

        <span className="files-toolbar__spacer" />

        {/* 常驻筛选条件（与搜索框同级；去掉「筛选」展开按钮，一次点击即筛选） */}
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

        {/* 主操作：整理选中（右侧，与检索条件同组） */}
        <button
          type="button"
          className="btn"
          onClick={handleClassifySelected}
          disabled={selectedIds.length === 0}
        >
          整理选中 ({selectedIds.length})
        </button>
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
