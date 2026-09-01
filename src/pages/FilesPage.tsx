// 文件管理主页：虚拟滚动列表 + 筛选排序 + 预览抽屉（设计稿 05_交互原型 §文件管理）。
//
// 结构：main-header(h1 + subtitle + header-actions) → main-content(file-toolbar + file-table)
// 筛选/排序为页面级 state（不污染 store）；选中与文件数据走 fileStore。

import { useDeferredValue, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { open } from '@tauri-apps/plugin-dialog';

import { FileListTable } from '@/components/file/FileListTable';
import { LazyFilePreviewDrawer } from '@/components/common/LazyFilePreviewDrawer';
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
import { useSettingsStore } from '@/stores/settingsStore';
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
  const dataDirectory = useSettingsStore((s) => s.dataDirectory);
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

  // 搜索用 deferred 值驱动列表重算：输入框更新（高优先级）不被
  // 全量 O(N log N) 过滤+排序（低优先级）阻塞，万级文件下逐字输入不再卡顿
  const deferredQuery = useDeferredValue(searchQuery);

  const visibleFiles = useMemo(() => {
    const filtered = filterFiles(files, {
      category: categoryFilter || null,
      status: statusFilter || null,
    });
    // 客户端搜索（对齐交互原型搜索框，数据全量在内存）
    return sortFiles(filterByName(filtered, deferredQuery), sort.key, sort.dir);
  }, [files, categoryFilter, statusFilter, deferredQuery, sort]);

  // T9.5 E2E：测试目录存在时跳过原生对话框（原生 open() 无法被 WebDriver 点击）。
  const handleScan = async () => {
    const testDir = await getE2eTestDir();
    if (testDir) {
      await scanFiles(testDir);
      return;
    }
    const dialogOptions: Parameters<typeof open>[0] = {
      directory: true,
      multiple: false,
      title: '选择要管理的目录',
    };
    if (dataDirectory) {
      dialogOptions.defaultPath = dataDirectory;
    }
    const selected = await open(dialogOptions);
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

  const handleClearSelection = () => {
    clearSelection();
    // 清除选中语义上等价于放弃"这批待分类文件"，同步作废分类页的旧预览缓存，
    // 否则用户返回分类页会看到上一批 12k+ 文件的旧预览树，误以为选中还残留。
    useClassifyStore.getState().reset();
  };

  const handleClassifySelected = () => {
    useClassifyStore.getState().reset();
    navigate('/classify');
  };

  return (
    <div className="page files-page">
      <header className="main-header">
        <h1>文件管理</h1>
        {scanPath && (
          <span className="subtitle" title={scanPath}>
            {scanPath}
          </span>
        )}
        <div className="header-actions">
          {/* FE-C4：刷新也进 isScanning 态（store），狂点被防抖 */}
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            onClick={() => void loadAllFiles()}
            disabled={isScanning}
          >
            刷新
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
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

      {/* 文件页主体：虚拟滚动自带滚动容器，不用 main-content 的 overflow */}
      <div className="files-page__body">
        <div className="file-toolbar">
          {/* 工具栏顺序对齐文档：整理选中 → 搜索框 → 筛选 */}
          <button
            type="button"
            className="btn btn--sm"
            onClick={handleClassifySelected}
            disabled={selectedIds.length === 0}
          >
            整理选中 ({selectedIds.length})
          </button>
          {selectedIds.length > 0 && (
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={handleClearSelection}
              data-testid="files-clear-selection"
            >
              清除选中
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
        <LazyFilePreviewDrawer
          key={previewFile?.id}
          file={previewFile}
          onClose={() => setPreviewFile(null)}
        />
      </div>
    </div>
  );
}
