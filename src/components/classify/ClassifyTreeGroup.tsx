// 分类预览树的单个分组面板：组头（折叠箭头 + 组名 + 全选 + 计数）+ 组内文件行。
//
// 从 ClassifyPreviewTree 按视觉区域拆出：组头（折叠交互的触发区）由
// ClassifyTreeGroupHeader 渲染，组内文件行由 ClassifyTreeNode 渲染；父组件只负责
// 分组与勾选/折叠状态，本组件不改动这些语义。

import type { PreviewGroup } from '@/lib/classifyTree';
import type { ClassifyPlanItem } from '@/types/ipc';

import { ClassifyBatchAssignBar } from './ClassifyBatchAssignBar';
import { ClassifyTreeGroupHeader } from './ClassifyTreeGroupHeader';
import { ClassifyTreeNode } from './ClassifyTreeNode';
import type { ClassifyTreeController } from './useClassifyTreeState';

interface ClassifyTreeGroupProps {
  /** 本分组（组名 + 文件项） */
  group: PreviewGroup;
  /** 是否已折叠（默认全展开） */
  collapsed: boolean;
  /** 是否为待确认组（全选、批量横幅、单文件指定分类仅此组出现） */
  pending: boolean;
  /** 是否为冲突组（展示「需处理」与冲突标记） */
  conflict: boolean;
  /** 目标子目录展示用的收纳根 */
  outputRoot: string | null;
  /** 勾选/折叠/指定分类的交互面 */
  tree: ClassifyTreeController;
  /** 点击文件名打开预览抽屉（缺省时文件名渲染为纯文本） */
  onOpenPreview?: (item: ClassifyPlanItem) => void;
}

export function ClassifyTreeGroup({
  group,
  collapsed,
  pending,
  conflict,
  outputRoot,
  tree,
  onOpenPreview,
}: ClassifyTreeGroupProps) {
  const groupContext = { pending, conflict, outputRoot };
  const nodeActions = {
    categories: tree.categories,
    onToggleSelect: tree.toggleSelect,
    onAssign: tree.assignOne,
    ...(onOpenPreview ? { onOpenPreview } : {}),
  };

  return (
    <div className={`tree-panel${pending ? ' confirm' : ''}${conflict ? ' conflict' : ''}`}>
      <ClassifyTreeGroupHeader
        name={group.name}
        count={group.items.length}
        conflict={conflict}
        collapse={{ collapsed, onToggle: () => tree.toggleGroup(group.name) }}
        selectAll={{
          visible: pending,
          checked: tree.allSelected,
          onToggle: tree.toggleSelectAll,
        }}
      />

      {!collapsed && (
        <div className="tree-children">
          {/* 批量指定分类横幅（待确认组勾选 ≥1 时显示） */}
          {pending && tree.selectedIds.length > 0 && (
            <ClassifyBatchAssignBar
              selectedCount={tree.selectedIds.length}
              categories={tree.categories}
              onAssign={tree.assignSelected}
              onClear={tree.clearSelection}
            />
          )}

          {group.items.map((item) => (
            <ClassifyTreeNode
              key={item.file_id}
              item={item}
              group={groupContext}
              selectedIds={tree.selectedIds}
              actions={nodeActions}
            />
          ))}
        </div>
      )}
    </div>
  );
}
