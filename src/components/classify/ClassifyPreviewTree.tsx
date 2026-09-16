// 分类预览树（对齐交互原型 §智能分类）：树形可折叠分组预览。
//
// 分组规则（对齐原型）：
//   - 分类组：`category_name` 非空且非冲突 → 按分类名分组（📁）
//   - 冲突组：`status=Conflict` → 独立「⚠️ 冲突」组（执行时按策略跳过）
//   - 待确认组：`category_name` 为空 → 「❓ 待确认」组（支持批量/单个手动指定分类）
//
// 按视觉区域拆分，本文件只做「分组 + 交互面编排」：
//   @/lib/classifyTree            分组、组名常量、目标子目录文案（纯函数）
//   ./useClassifyTreeState.ts     勾选/折叠/指定分类状态
//   ./ClassifyTreeGroup.tsx       分组面板（组头 + 批量横幅 + 文件行）
//   ./ClassifyTreeGroupHeader.tsx 分组面板组头（折叠箭头 + 组名 + 全选 + 计数）
//   ./ClassifyTreeNode.tsx        单行文件（勾选/文件名/标签/指定分类）
//   ./ClassifyBatchAssignBar.tsx  批量指定分类横幅
//   ./ClassifyCategorySelect.tsx  「指定分类」下拉（批量与单文件共用）

import { useMemo } from 'react';

import { CONFLICT_NAME, groupPreviewItems, PENDING_LABEL } from '@/lib/classifyTree';
import { useFileStore } from '@/stores/fileStore';
import type { ClassifyPlanItem, ClassifyPreview } from '@/types/ipc';

import { ClassifyTreeGroup } from './ClassifyTreeGroup';
import { useClassifyTreeState } from './useClassifyTreeState';

// 路径边界校验的展示辅助（实现见 @/lib/classifyTree；对外导出路径保持不变）
export { targetSubdir } from '@/lib/classifyTree';

interface ClassifyPreviewTreeProps {
  /** 分类预览结果 */
  preview: ClassifyPreview;
  /** 点击文件名打开预览抽屉（由父级传入对应文件的最小元信息） */
  onOpenPreview?: (item: ClassifyPlanItem) => void;
}

export function ClassifyPreviewTree({ preview, onOpenPreview }: ClassifyPreviewTreeProps) {
  const groups = useMemo(() => groupPreviewItems(preview.items), [preview.items]);
  const scanPath = useFileStore((s) => s.scanPath);
  const pendingGroup = groups.find((g) => g.name === PENDING_LABEL);
  const tree = useClassifyTreeState((pendingGroup?.items ?? []).map((i) => i.file_id));

  return (
    <div className="classify-preview">
      <div className="classify-tree-header">
        <h3>分类预览</h3>
        <span className="count">
          {/* FE-M10：从 items 现算——stats.by_* 是预览时点快照，漏手动分配 */}
          {preview.items.filter((i) => i.category_name != null).length} 个已分类 ·{' '}
          {preview.items.filter((i) => i.category_name == null).length} 个待确认
        </span>
      </div>

      <div className="classify-tree">
        {groups.map((group) => (
          <ClassifyTreeGroup
            key={group.name}
            group={group}
            collapsed={tree.collapsed[group.name] === true}
            pending={group.name === PENDING_LABEL}
            conflict={group.name === CONFLICT_NAME}
            outputRoot={preview.output_root || scanPath}
            tree={tree}
            {...(onOpenPreview ? { onOpenPreview } : {})}
          />
        ))}
      </div>
    </div>
  );
}
