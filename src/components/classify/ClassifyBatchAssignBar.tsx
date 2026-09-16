// 批量指定分类横幅（待确认组勾选 ≥1 时显示）：已选计数 + 分类下拉 + 取消。

import type { Category } from '@/types/ipc';

import { ClassifyCategorySelect } from './ClassifyCategorySelect';

interface ClassifyBatchAssignBarProps {
  /** 已勾选文件数 */
  selectedCount: number;
  /** 分类下拉数据源 */
  categories: Category[];
  /** 为已勾选文件批量指定分类 */
  onAssign: (categoryName: string) => void;
  /** 清空勾选（关闭横幅） */
  onClear: () => void;
}

export function ClassifyBatchAssignBar({
  selectedCount,
  categories,
  onAssign,
  onClear,
}: ClassifyBatchAssignBarProps) {
  return (
    <div className="classify-preview__batch">
      <span>已选 {selectedCount} 个文件</span>
      <ClassifyCategorySelect
        className="classify-preview__assign"
        label="为选中的文件批量指定分类"
        categories={categories}
        onChange={onAssign}
      />
      <button type="button" className="classify-preview__batch-cancel" onClick={onClear}>
        取消
      </button>
    </div>
  );
}
