// 分类预览里的文件预览抽屉（从 ClassifyPage 的 previewTarget 与 handleOpenPreviewItem 抽出）。
//
// 树中的计划项只带 original_path/file_name/category_name，这里映射成抽屉要的预览目标。

import { useState } from 'react';

import type { FilePreviewTarget } from '@/components/common/FilePreviewDrawer';
import type { ClassifyPlanItem } from '@/types/ipc';

/** `useClassifyPreviewDrawer` 的对外出口。 */
export interface ClassifyPreviewDrawer {
  /** 抽屉预览目标（null 表示关闭） */
  previewTarget: FilePreviewTarget | null;
  /** 打开树中某个计划项的预览 */
  openPreviewItem: (item: ClassifyPlanItem) => void;
  /** 关闭抽屉 */
  closePreview: () => void;
}

/** 管理分类预览左侧树点击文件名时的预览抽屉目标。 */
export function useClassifyPreviewDrawer(): ClassifyPreviewDrawer {
  const [previewTarget, setPreviewTarget] = useState<FilePreviewTarget | null>(null);

  const openPreviewItem = (item: ClassifyPlanItem) => {
    setPreviewTarget({
      path: item.original_path,
      file_name: item.file_name,
      category: item.category_name,
    });
  };

  return {
    previewTarget,
    openPreviewItem,
    closePreview: () => setPreviewTarget(null),
  };
}
