// 分类预览的发起与取消（从 ClassifyPage 的自动预览 effect、handleStart、handleCancelPreview 抽出）。
//
// 自动预览：从文件页「整理选中」跳转进入时带选中态，挂载即生成一次预览（仅首次挂载）。
// 软排除：无选中时只处理未整理文件，全部已整理则提示而非发空请求。

import { useEffect } from 'react';

import { isOrganized } from '@/lib/fileTable';
import { useClassifyStore } from '@/stores/classifyStore';
import { useFileStore } from '@/stores/fileStore';
import { ClassifyStatus } from '@/types/models';

/** `useClassifyPreviewFlow` 的对外出口。 */
export interface ClassifyPreviewFlow {
  /** 开始分类（生成预览） */
  start: () => void;
  /** 取消并丢弃当前预览 */
  cancelPreview: () => void;
}

/** 管理分类预览的自动发起、手动发起与取消。 */
export function useClassifyPreviewFlow(): ClassifyPreviewFlow {
  const generatePreview = useClassifyStore((s) => s.generatePreview);
  const reset = useClassifyStore((s) => s.reset);

  useEffect(() => {
    const { selectedIds } = useFileStore.getState();
    const { status, preview } = useClassifyStore.getState();
    if (selectedIds.length > 0 && status === ClassifyStatus.Idle && preview === null) {
      void generatePreview(selectedIds);
    }
  }, [generatePreview]);

  /** 开始分类：有选中尊重选中（可手动重选已整理文件）；无选中仅处理未整理文件 */
  const start = () => {
    const { files, selectedIds } = useFileStore.getState();
    const ids =
      selectedIds.length > 0 ? selectedIds : files.filter((f) => !isOrganized(f)).map((f) => f.id);
    if (ids.length === 0) {
      // 防御：全部已整理且无选中（按钮已禁用，正常不触发），提示而非发空请求
      useClassifyStore.setState({ error: '当前没有未整理的文件，无需分类' });
      return;
    }
    void generatePreview(ids);
  };

  /** 取消预览：同步清空文件页选中（FE-M9） */
  const cancelPreview = () => {
    // 否则残留的 selectedIds 在下次进入分类页时又触发自动生成
    useFileStore.getState().clearSelection();
    reset();
  };

  return { start, cancelPreview };
}
