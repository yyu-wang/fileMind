// 分类预览树的单行文件：勾选框（待确认组）、文件名、来源标签、冲突/目标子目录标记、
// 单文件指定分类下拉。
//
// 从 ClassifyPreviewTree 的渲染回调拆出：原回调同时承担分组面板与文件行两层渲染
// （127 行），拆成组件后组内文件的显隐判定仍按所在组的上下文（group）决定。

import { targetSubdir } from '@/lib/classifyTree';
import type { ClassifyPlanItem, Category } from '@/types/ipc';

import { ClassifyCategorySelect } from './ClassifyCategorySelect';
import { ClassifyRuleSourceTag } from './ClassifyRuleSourceTag';

interface ClassifyTreeNodeProps {
  /** 文件项 */
  item: ClassifyPlanItem;
  /** 所在组的上下文：决定勾选框/冲突标记/目标子目录的显隐 */
  group: { pending: boolean; conflict: boolean; outputRoot: string | null };
  /** 已勾选文件 id（仅待确认组渲染勾选框） */
  selectedIds: string[];
  /** 交互动作 */
  actions: {
    categories: Category[];
    onToggleSelect: (fileId: string) => void;
    onAssign: (fileId: string, categoryName: string) => void;
    onOpenPreview?: (item: ClassifyPlanItem) => void;
  };
}

export function ClassifyTreeNode({ item, group, selectedIds, actions }: ClassifyTreeNodeProps) {
  const { pending, conflict, outputRoot } = group;
  const { categories, onToggleSelect, onAssign, onOpenPreview } = actions;

  return (
    <div className="tree-node child">
      {/* 待确认组文件可勾选（批量分类） */}
      {pending && (
        <input
          type="checkbox"
          className="classify-preview__check"
          checked={selectedIds.includes(item.file_id)}
          onChange={() => onToggleSelect(item.file_id)}
          aria-label={`选择 ${item.file_name}`}
        />
      )}
      {/* 提供 onOpenPreview 时文件名可点击，打开预览抽屉（对齐文件页） */}
      {onOpenPreview ? (
        <button
          type="button"
          className="tree-node__name tree-node__name--preview"
          title={item.original_path}
          onClick={() => onOpenPreview(item)}
        >
          {item.file_name}
        </button>
      ) : (
        <span className="tree-node__name" title={item.original_path}>
          {item.file_name}
        </span>
      )}
      <ClassifyRuleSourceTag source={item.rule_source} />
      {conflict && <span className="tree-node__conflict">目标已存在（跳过）</span>}
      {!pending && !conflict && (
        <span className="tree-node__dir">{targetSubdir(item.target_path, outputRoot)}</span>
      )}
      {/* 待确认文件单个指定分类（对齐原型「确认/改分类」） */}
      {pending && categories.length > 0 && (
        <ClassifyCategorySelect
          className="classify-preview__assign"
          label={`为 ${item.file_name} 指定分类`}
          categories={categories}
          onChange={(categoryName) => onAssign(item.file_id, categoryName)}
        />
      )}
    </div>
  );
}
