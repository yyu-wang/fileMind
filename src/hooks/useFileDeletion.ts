// 删除选中文件的二次确认（从 FilesPage 的 pendingDelete / deleting / handleConfirmDelete 抽出）。
//
// 确认后由 store 把文件移入系统回收站，结束后清空整批选中；失败经 store.error 提示。

import { useState } from 'react';

import { useFileStore } from '@/stores/fileStore';

/** `useFileDeletion` 的对外出口。 */
export interface FileDeletionHandle {
  /** 删除确认弹窗是否打开 */
  pendingDelete: boolean;
  /** 删除请求进行中（弹窗按钮 loading） */
  deleting: boolean;
  /** 请求删除（打开确认弹窗） */
  requestDelete: () => void;
  /** 取消删除（关闭确认弹窗，不调用 IPC） */
  cancelDelete: () => void;
  /** 确认删除（void 包装，可直接作为弹窗回调） */
  confirmDelete: () => void;
}

/** 管理「删除选中文件」的确认态、进行态与执行流程。 */
export function useFileDeletion(): FileDeletionHandle {
  const selectedIds = useFileStore((s) => s.selectedIds);
  const deleteFiles = useFileStore((s) => s.deleteFiles);
  const clearSelection = useFileStore((s) => s.clearSelection);
  const [pendingDelete, setPendingDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);

  /** 确认删除：调用 store 移入系统回收站，结束后清空整批选中。 */
  const runDelete = async () => {
    setDeleting(true);
    try {
      await deleteFiles(selectedIds);
    } catch {
      // 全量失败：错误已写入 store.error（顶部横幅展示）
    } finally {
      clearSelection();
      setDeleting(false);
      setPendingDelete(false);
    }
  };

  return {
    pendingDelete,
    deleting,
    requestDelete: () => setPendingDelete(true),
    cancelDelete: () => setPendingDelete(false),
    confirmDelete: () => void runDelete(),
  };
}
