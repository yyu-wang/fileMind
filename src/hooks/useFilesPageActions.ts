// 文件页的动作与本地交互状态（刷新 / 扫描 / 选中 / 预览 / 删除）。
//
// 原先这些回调与状态都堆在 FilesPage 的页面函数里（217 行），页面只保留编排之后，
// 「点了会发生什么」集中在本 Hook：扫描走 lib/fileScan，预览与删除各自独立成 Hook。

import { useCallback } from 'react';
import { useNavigate } from 'react-router-dom';

import { useFileDeletion, type FileDeletionHandle } from '@/hooks/useFileDeletion';
import { useFilePreview, type FilePreviewHandle } from '@/hooks/useFilePreview';
import { useFilesBootstrap } from '@/hooks/useFilesBootstrap';
import { scanDirectoryViaDialog } from '@/lib/fileScan';
import { useClassifyStore } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import { useSettingsStore } from '@/stores/settingsStore';

/** `useFilesPageActions` 的返回值（含预览与删除两条子链路）。 */
export interface FilesPageActions extends FilePreviewHandle, FileDeletionHandle {
  /** 重新取回全量文件列表（头部「刷新」与目录面板移除后共用） */
  refresh: () => void;
  /** 扫描目录（E2E 测试目录优先，否则弹原生目录选择框） */
  scan: () => void;
  /** 关闭错误横幅 */
  clearError: () => void;
  toggleSelect: (id: string) => void;
  /** 表头全选/取消全选（null 表示清空选中） */
  selectAll: (ids: string[] | null) => void;
  /** 「清除选中」：清空选中并作废分类页旧预览 */
  clearSelection: () => void;
  /** 「整理选中」：重置分类页并跳转 */
  classifySelected: () => void;
}

/** 提供文件页的全部动作与交互状态。 */
export function useFilesPageActions(): FilesPageActions {
  const loadAllFiles = useFileStore((s) => s.loadAllFiles);
  const scanFiles = useFileStore((s) => s.scanFiles);
  const toggleSelect = useFileStore((s) => s.toggleSelect);
  const setSelection = useFileStore((s) => s.setSelection);
  const clearSelection = useFileStore((s) => s.clearSelection);
  const clearError = useFileStore((s) => s.clearError);
  const dataDirectory = useSettingsStore((s) => s.dataDirectory);
  const navigate = useNavigate();

  useFilesBootstrap();
  const preview = useFilePreview();
  const deletion = useFileDeletion();

  const refresh = useCallback(() => void loadAllFiles(), [loadAllFiles]);
  const scan = useCallback(
    () => void scanDirectoryViaDialog(scanFiles, dataDirectory),
    [scanFiles, dataDirectory],
  );

  const selectAll = (ids: string[] | null) => {
    if (ids === null) {
      clearSelection();
      return;
    }
    setSelection(ids);
  };

  const clearSelectionAndDiscardPreview = () => {
    clearSelection();
    // 清除选中语义上等价于放弃"这批待分类文件"，同步作废分类页的旧预览缓存，
    // 否则用户返回分类页会看到上一批 12k+ 文件的旧预览树，误以为选中还残留。
    useClassifyStore.getState().reset();
  };

  const classifySelected = () => {
    useClassifyStore.getState().reset();
    navigate('/classify');
  };

  return {
    refresh,
    scan,
    clearError,
    toggleSelect,
    selectAll,
    clearSelection: clearSelectionAndDiscardPreview,
    classifySelected,
    ...preview,
    ...deletion,
  };
}
