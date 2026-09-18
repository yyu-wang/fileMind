// 分类预览树的交互状态：批量勾选、折叠态与「指定分类」动作。
//
// 抽成 Hook 的原因（complexity 规则 §React 组件拆分决策树）：勾选/折叠/单文件与批量
// 指定分类共 5 组回调原先都堆在 ClassifyPreviewTree 里，把组件推到函数行数阈值之上；
// 状态与回调独立后，组件回到「分组 → 渲染」，勾选语义也可单独演进。

import { useState } from 'react';

import { useClassifyStore } from '@/stores/classifyStore';
import type { Category } from '@/types/ipc';

/** 预览树的交互面（树容器与分组面板按需取用）。 */
export interface ClassifyTreeController {
  /** 已勾选的待确认文件 id */
  selectedIds: string[];
  /** 待确认组是否已全选 */
  allSelected: boolean;
  /** 折叠态：组名 → 是否折叠（默认全展开） */
  collapsed: Record<string, boolean>;
  /** 分类下拉数据源 */
  categories: Category[];
  /** 切换某组的折叠态 */
  toggleGroup: (name: string) => void;
  /** 切换单个文件的勾选态 */
  toggleSelect: (fileId: string) => void;
  /** 待确认组全选 / 取消全选 */
  toggleSelectAll: () => void;
  /** 清空勾选（批量横幅的「取消」） */
  clearSelection: () => void;
  /** 单个文件指定分类 */
  assignOne: (fileId: string, categoryName: string) => void;
  /** 为已勾选文件批量指定分类 */
  assignSelected: (categoryName: string) => void;
}

/**
 * 生成预览树的交互面。
 *
 * Args:
 *   pendingFileIds: 待确认组的文件 id（批量勾选的作用范围）
 *
 * Returns:
 *   勾选/折叠状态与对应动作
 */
export function useClassifyTreeState(pendingFileIds: string[]): ClassifyTreeController {
  // 手动分类数据源 + 动作（待确认组用）
  const categories = useClassifyStore((s) => s.categories);
  const assignCategory = useClassifyStore((s) => s.assignCategory);
  const assignCategories = useClassifyStore((s) => s.assignCategories);
  // 批量勾选态（仅待确认组内未分类项可勾选，纯 UI 状态）
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  // 树折叠态：组名 → 是否折叠（默认全展开）
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const allSelected =
    pendingFileIds.length > 0 && pendingFileIds.every((id) => selectedIds.includes(id));

  /** 按名取分类（下拉只回传分类名）。 */
  const findCategory = (name: string): Category | undefined =>
    categories.find((c) => c.name === name);

  return {
    selectedIds,
    allSelected,
    collapsed,
    categories,
    toggleGroup: (name) => setCollapsed((prev) => ({ ...prev, [name]: !prev[name] })),
    toggleSelect: (fileId) =>
      setSelectedIds((prev) =>
        prev.includes(fileId) ? prev.filter((id) => id !== fileId) : [...prev, fileId],
      ),
    toggleSelectAll: () => setSelectedIds(allSelected ? [] : pendingFileIds),
    clearSelection: () => setSelectedIds([]),
    assignOne: (fileId, categoryName) => {
      const category = findCategory(categoryName);
      if (category) assignCategory(fileId, category);
    },
    assignSelected: (categoryName) => {
      const category = findCategory(categoryName);
      if (category && selectedIds.length > 0) {
        assignCategories(selectedIds, category);
        setSelectedIds([]);
      }
    },
  };
}
