// 文件预览抽屉的状态与 Space 快捷键（T6.10）。
//
// 原先这块状态、快捷键与回调都内联在 FilesPage 里，页面函数只剩编排——
// 预览目标是纯页面级 state（不进 store），开关与快捷键同属一条闭环，收在这里。

import { useState } from 'react';

import { useHotkeys } from '@/hooks/useHotkeys';
import { useFileStore } from '@/stores/fileStore';
import type { FileInfo } from '@/types/ipc';

/** `useFilePreview` 的对外出口。 */
export interface FilePreviewHandle {
  /** 当前预览目标，null 表示抽屉关闭 */
  previewFile: FileInfo | null;
  /** 打开预览（表格行点击 / Space 快捷键） */
  openPreview: (file: FileInfo) => void;
  /** 关闭预览抽屉 */
  closePreview: () => void;
}

/** 管理文件预览抽屉的目标与开关，并注册 Space 快捷键。 */
export function useFilePreview(): FilePreviewHandle {
  const files = useFileStore((s) => s.files);
  const selectedIds = useFileStore((s) => s.selectedIds);
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

  const openPreview = (file: FileInfo) => setPreviewFile(file);
  const closePreview = () => setPreviewFile(null);

  return { previewFile, openPreview, closePreview };
}
